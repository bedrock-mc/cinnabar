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
        const ch = await adapter.open(t, values.RELAY_CHANNEL_PLAY); const ws = Socket.instances.at(-1);
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
    await assert.rejects(adapter.open(t, values.RELAY_CHANNEL_PLAY));
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
