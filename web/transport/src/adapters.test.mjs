import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import * as adapter from "./adapters.mjs";

const contract = JSON.parse(await readFile(new URL("../../../crates/bridge/data/contract.json", import.meta.url)));
const values = Object.fromEntries(contract.constants.map(c => [c.rust_name, c.value]));
const config = {
    transport: values.RELAY_TRANSPORT_PATH, controlPath: values.RELAY_CONTROL_PATH,
    playPath: values.RELAY_PLAY_PATH, control: values.RELAY_CHANNEL_CONTROL,
    protocol: values.RELAY_SOCKET_PROTOCOL, controlBytes: values.RELAY_CONTROL_BYTES,
    playBytes: values.RELAY_PLAY_MESSAGE_BYTES, chunks: values.QUEUED_FRAMES,
};

/** Advances promise continuations without using wall-clock timers. */
async function turns() { for (let i = 0; i < 8; i++) await Promise.resolve(); }
/** Supplies observable WebSocket byte delivery and closure on the browser event loop. */
class Socket {
    static OPEN = 1;
    static instances = [];
    constructor(url, protocol) {
        Object.assign(this, { url, protocol, readyState: Socket.OPEN, bufferedAmount: 0, sent: [] });
        Socket.instances.push(this); queueMicrotask(() => this.onopen?.());
    }
    /** Retains bytes accepted for sending; tests control when they drain. */
    send(bytes) { this.sent.push(bytes); this.bufferedAmount += bytes.byteLength; }
    /** Delivers a normal close notification. */
    close() { this.readyState = 3; this.onclose?.(); }
}
/** Supplies QUIC stream promises and observable lock release without network access. */
class Quic {
    static instances = [];
    constructor(url) {
        this.url = url; this.ready = Promise.resolve(); this.closed = new Promise(() => {});
        this.sent = []; this.released = 0; this.closes = 0;
        this.incomingUnidirectionalStreams = new ReadableStream({ start: controller => { this.incoming = controller; } });
        this.stream = {
            readable: { getReader: () => ({ read: async () => ({ value: new Uint8Array([1, 2]), done: false }),
                cancel: async () => {}, releaseLock: () => { this.released++; } }) },
            writable: { getWriter: () => ({ ready: Promise.resolve(), write: async bytes => { this.sent.push(bytes); },
                abort: async () => {}, releaseLock: () => { this.released++; } }) },
        };
        Quic.instances.push(this);
    }
    /** Returns the fixture's next reliable stream. */
    async createBidirectionalStream() { return this.stream; }
    /** Records transport cancellation. */
    close() { this.closes++; }
}
globalThis.WebSocket = Socket; globalThis.WebTransport = Quic; globalThis.isSecureContext = true;

/** Keeps retired streams charged until the test delivers replacement stream credit. */
function delayStreamCredit(peer, available) {
    const waiting = [];
    let retired = 0;
    /** Charges a distinct stream and records retirement only after both directions close. */
    function stream() {
        available--;
        let closed = 0;
        /** Retires credit after cancellation reaches both stream directions. */
        const retire = async () => { if (++closed === 2) retired++; };
        return {
            readable: { getReader: () => ({ ...peer.stream.readable.getReader(), cancel: retire }) },
            writable: { getWriter: () => ({ ...peer.stream.writable.getWriter(), abort: retire }) },
        };
    }
    peer.createBidirectionalStream = async (options = {}) => {
        if (available) return stream();
        if (!options.waitUntilAvailable) throw new DOMException("stream credit exhausted", "QuotaExceededError");
        return new Promise(resolve => waiting.push(resolve));
    };
    return {
        get pending() { return waiting.length; },
        /** Delivers credit separately from local stream cancellation. */
        replenish() {
            assert.ok(retired > 0); retired--; available++;
            if (waiting.length) waiting.shift()(stream());
        },
    };
}

test("QUIC rejoin waits for delayed stream credit while retaining control", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    const peer = Quic.instances.at(-1), credit = delayStreamCredit(peer, 2);
    try {
        const control = await adapter.open(t, config.control);
        await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
        adapter.finishPlay(t);
        const opening = adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
        let settled = false; opening.then(() => { settled = true; }, () => { settled = true; });
        await turns();
        assert.equal(settled, false); assert.equal(credit.pending, 1);
        assert.deepEqual(await adapter.read(control), new Uint8Array([1, 2]));
        await adapter.write(control, new Uint8Array([3]));
        credit.replenish();
        const play = await opening;
        assert.deepEqual(await adapter.read(play), new Uint8Array([1, 2]));
        assert.equal(peer.closes, 0); assert.equal(control.failed, undefined);
    } finally { adapter.close(t); }
});

test("cancelling a QUIC stream-credit wait rejects promptly and releases a late stream", async () => {
    for (const closeTransport of [false, true]) {
        const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
        const peer = Quic.instances.at(-1), credit = delayStreamCredit(peer, 2);
        try {
            const control = await adapter.open(t, config.control);
            await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
            adapter.finishPlay(t);
            const opening = adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
            const rejected = opening.then(() => assert.fail("cancelled open succeeded"), error => error);
            await turns(); assert.equal(credit.pending, 1);
            if (closeTransport) adapter.close(t); else adapter.finishPlay(t);
            assert.match((await rejected).message, /relay closed/); await turns();
            const released = peer.released;
            credit.replenish(); await turns();
            assert.equal(peer.released, released + 2);
            assert.equal(t.channels.size, closeTransport ? 0 : 1);
            if (!closeTransport) {
                assert.deepEqual(await adapter.read(control), new Uint8Array([1, 2]));
                assert.equal(peer.closes, 0);
            }
        } finally { adapter.close(t); }
    }
});

test("both adapters preserve byte chunks and release their owned handles", async () => {
    for (const quic of [true, false]) {
        const transport = adapter.create("https://relay.invalid", quic, JSON.stringify(config));
        const channel = await adapter.open(transport, config.control);
        if (!quic) Socket.instances.at(-1).onmessage({ data: new Uint8Array([1, 2]).buffer });
        assert.deepEqual(await adapter.read(channel), new Uint8Array([1, 2]));
        await adapter.write(channel, new Uint8Array([3, 4]));
        const peer = quic ? Quic.instances.at(-1) : Socket.instances.at(-1);
        assert.deepEqual(peer.sent[0], new Uint8Array([3, 4]));
        if (quic) {
            assert.equal(peer.url.pathname, config.transport);
        } else {
            assert.equal(peer.url.pathname, config.controlPath);
            peer.bufferedAmount = 0; await adapter.flush(channel);
        }
        adapter.close(transport); await turns();
        assert.equal(quic ? peer.released : peer.readyState, quic ? 2 : 3);
    }
});

test("WebSocket receive bounds reject oversized, over-budget and excessive messages before copying", async () => {
    for (const sizes of [[config.controlBytes + 5], [config.controlBytes, 5], Array(config.chunks + 1).fill(1)]) {
        const t = adapter.create("https://relay.invalid", false, JSON.stringify(config));
        const ch = await adapter.open(t, config.control); const ws = Socket.instances.at(-1);
        for (const size of sizes) ws.onmessage({ data: new ArrayBuffer(size) });
        assert.equal(ws.readyState, 3);
        await assert.rejects(adapter.write(ch, new Uint8Array([1])));
        assert.equal(ws.sent.length, 0);
        await assert.rejects(adapter.read(ch));
    }
});

test("a blocked send re-arms its timer and closure cancels it immediately", async () => {
    const timers = new Map(); let id = 0;
    const originalSet = globalThis.setTimeout, originalClear = globalThis.clearTimeout;
    globalThis.setTimeout = resolve => { timers.set(++id, resolve); return id; };
    globalThis.clearTimeout = key => timers.delete(key);
    try {
        const t = adapter.create("https://relay.invalid", false, JSON.stringify(config));
        const ch = await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1"); const ws = Socket.instances.at(-1);
        ws.bufferedAmount = config.playBytes;
        const writing = adapter.write(ch, new Uint8Array([1]));
        for (let i = 0; i < 3; i++) {
            assert.equal(timers.size, 1); [...timers.values()][0](); await turns();
            assert.equal(ws.sent.length, 0);
        }
        ws.bufferedAmount = config.playBytes - 1; [...timers.values()][0](); await writing;
        assert.equal(ws.bufferedAmount, config.playBytes); assert.equal(timers.size, 0);
        const flushing = adapter.flush(ch); assert.equal(timers.size, 1);
        adapter.close(t); await assert.rejects(flushing); assert.equal(timers.size, 0);
    } finally { globalThis.setTimeout = originalSet; globalThis.clearTimeout = originalClear; }
});

test("QUIC cancellation owns late streams and interrupts writer readiness", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config)); const peer = Quic.instances.at(-1);
    let resolve; peer.createBidirectionalStream = () => new Promise(yes => { resolve = yes; });
    const opening = adapter.open(t, config.control); await turns(); adapter.close(t);
    await assert.rejects(opening); resolve(peer.stream); await turns(); assert.equal(peer.released, 2);
    const next = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    const ch = await adapter.open(next, config.control); ch.writer.ready = new Promise(() => {});
    const writing = adapter.write(ch, new Uint8Array([1])); adapter.close(next);
    await assert.rejects(writing); assert.equal(Quic.instances.at(-1).sent.length, 0);
});

test("shutdown waits for accepted QUIC writes and closes gracefully", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    const ch = await adapter.open(t, config.control);
    let ready, finishWrite; let closes = 0, aborts = 0;
    ch.writer.ready = new Promise(resolve => { ready = resolve; });
    ch.writer.write = () => new Promise(resolve => { finishWrite = resolve; });
    ch.writer.close = async () => { closes++; };
    ch.writer.abort = async () => { aborts++; };
    const writing = adapter.write(ch, new Uint8Array([1]));
    let finished = false;
    const stopping = adapter.shutdown(ch).then(() => { finished = true; });
    await turns(); assert.equal(finished, false); assert.equal(closes, 0);
    ready(); await turns(); assert.equal(finished, false); assert.equal(closes, 0);
    finishWrite(); await writing; await stopping;
    assert.equal(closes, 1); assert.equal(aborts, 0);
    adapter.closeChannel(ch); await turns(); assert.equal(aborts, 0);
});

test("closed candidates reject channels without creating browser handles", async () => {
    for (const quic of [false, true]) {
        const t = adapter.create("https://relay.invalid", quic, JSON.stringify(config));
        const peer = quic ? Quic.instances.at(-1) : undefined;
        let streams = 0;
        if (peer) peer.createBidirectionalStream = async () => { streams++; return peer.stream; };
        adapter.close(t);
        const sockets = Socket.instances.length;
        await assert.rejects(adapter.open(t, config.control));
        assert.equal(Socket.instances.length, sockets); assert.equal(streams, 0);
        assert.equal(t.channels.size, 0);
    }
    const t = adapter.create("https://relay.invalid", false, JSON.stringify(config));
    await adapter.open(t, config.control);
    Socket.instances.at(-1).onmessage({ data: new ArrayBuffer(config.controlBytes + 5) });
    const sockets = Socket.instances.length;
    await assert.rejects(adapter.open(t, values.RELAY_CHANNEL_PLAY, "1"));
    assert.equal(Socket.instances.length, sockets);
});

test("WebSocket writes reject cancellation after an unthrottled drain", async () => {
    for (const cancelTransport of [true, false]) {
        const t = adapter.create("https://relay.invalid", false, JSON.stringify(config));
        const ch = await adapter.open(t, config.control);
        const ws = Socket.instances.at(-1);
        const writing = adapter.write(ch, new Uint8Array([1]));
        if (cancelTransport) adapter.close(t); else ws.close();
        await assert.rejects(writing);
        assert.equal(ws.sent.length, 0);
        adapter.close(t);
    }
});

/** Emits an archive stream with observable reset and lock ownership. */
function archiveStream(payload, join = 1n) {
    const bytes = new Uint8Array(8 + payload.length);
    new DataView(bytes.buffer).setBigUint64(0, join); bytes.set(payload, 8);
    let resets = 0;
    const stream = new ReadableStream({ start(controller) { controller.enqueue(new Uint8Array(bytes)); },
        cancel() { resets++; } });
    return { stream, bytes, resets: () => resets };
}

test("pack streams stay independent from control and play and reset between joins", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    const control = await adapter.open(t, config.control);
    const play = await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
    const peer = Quic.instances.at(-1);
    const archive = archiveStream([3, 4]);
    peer.incoming.enqueue(archive.stream);
    const packs = await adapter.acceptPacks(t);
    assert.deepEqual(await adapter.read(packs), archive.bytes);
    assert.deepEqual(await adapter.read(play), new Uint8Array([1, 2]));
    const extra = archiveStream([5]); peer.incoming.enqueue(extra.stream); await turns();
    assert.equal(extra.resets(), 1);
    const reading = adapter.read(packs);
    adapter.finishPlay(t); await assert.rejects(reading); await turns();
    assert.equal(archive.resets(), 1); assert.equal(archive.stream.locked, false);
    assert.deepEqual(await adapter.read(control), new Uint8Array([1, 2]));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
    const next = archiveStream([6], 2n); peer.incoming.enqueue(next.stream);
    const fresh = await adapter.acceptPacks(t);
    assert.notEqual(fresh, packs);
    assert.deepEqual(await adapter.read(fresh), next.bytes);
    adapter.close(t); await turns(); assert.equal(next.resets(), 1);
});

test("pending pack acceptance cancels immediately and fallback opens no bulk connection", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
    const accepting = adapter.acceptPacks(t);
    adapter.finishPlay(t); await assert.rejects(accepting);
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
    const archive = archiveStream([7], 2n); Quic.instances.at(-1).incoming.enqueue(archive.stream);
    assert.deepEqual(await adapter.read(await adapter.acceptPacks(t)), archive.bytes);
    adapter.close(t);
    const ws = adapter.create("https://relay.invalid", false, JSON.stringify(config));
    const sockets = Socket.instances.length;
    assert.equal(await adapter.acceptPacks(ws), null);
    assert.equal(Socket.instances.length, sockets);
    adapter.close(ws);
});

test("an unpolled old pack wait cannot claim the next join", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
    const stale = adapter.packChannel(t);
    adapter.finishPlay(t);
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
    await assert.rejects(adapter.acceptPacks(t, stale));
    const archive = archiveStream([8], 2n); Quic.instances.at(-1).incoming.enqueue(archive.stream);
    assert.deepEqual(await adapter.read(await adapter.acceptPacks(t)), archive.bytes);
    adapter.close(t);
});

test("a retired archive arriving after rejoin cannot claim the current pack channel", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
    adapter.finishPlay(t);
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "2");
    const stale = archiveStream([1], 1n), fresh = archiveStream([2], 2n);
    const peer = Quic.instances.at(-1);
    peer.incoming.enqueue(stale.stream);
    peer.incoming.enqueue(fresh.stream);
    const packs = await adapter.acceptPacks(t);
    assert.deepEqual(await adapter.read(packs), fresh.bytes);
    await turns();
    assert.equal(stale.resets(), 1); assert.equal(stale.stream.locked, false);
    adapter.close(t);
});

test("fragmented prefixes preserve bytes and a stalled retired stream cannot block a fresh join", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    const join = 9007199254740993n;
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, String(join));
    let resets = 0;
    const stalled = new ReadableStream({ start(c) { c.enqueue(new Uint8Array([0])); },
        cancel() { resets++; } });
    const encoded = archiveStream([4, 5], join).bytes;
    const fresh = new ReadableStream({ start(c) {
        for (const chunk of [encoded.slice(0, 3), encoded.slice(3, 8), encoded.slice(8)]) c.enqueue(chunk);
        c.close();
    } });
    const peer = Quic.instances.at(-1);
    peer.incoming.enqueue(stalled); peer.incoming.enqueue(fresh);
    const packs = await adapter.acceptPacks(t);
    const delivered = [];
    for (let bytes; (bytes = await adapter.read(packs)) !== null;) delivered.push(...bytes);
    assert.deepEqual(new Uint8Array(delivered), encoded);
    await turns(); assert.equal(resets, 1); assert.equal(stalled.locked, false);
    adapter.close(t);
});

test("finishing a join resets readers still waiting for their archive prefix", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "1");
    const partial = new ReadableStream({ start(c) { c.enqueue(new Uint8Array([0])); } });
    Quic.instances.at(-1).incoming.enqueue(partial); await turns();
    assert.equal(partial.locked, true);
    const accepting = adapter.acceptPacks(t);
    adapter.finishPlay(t); await assert.rejects(accepting); await turns();
    assert.equal(partial.locked, false);
    adapter.close(t);
});


test("retired prefix probes backpressure stream admission without resetting the fresh join", async () => {
    const t = adapter.create("https://relay.invalid", true, JSON.stringify(config));
    await adapter.open(t, values.RELAY_CHANNEL_PLAY, "3");
    const fresh = archiveStream([3], 3n);
    const peer = Quic.instances.at(-1);
    for (const archive of [archiveStream([1], 1n), archiveStream([2], 2n), fresh]) peer.incoming.enqueue(archive.stream);
    await turns();
    try {
        assert.equal(fresh.resets(), 0);
        assert.deepEqual(await adapter.read(await adapter.acceptPacks(t)), fresh.bytes);
    } finally { adapter.close(t); }
});
