// Browser handles stay on the event loop. Rust owns framing, session policy and copies.

/** Creates a candidate without awaiting readiness, so its owner can cancel a failed attempt. */
export function create(origin, quic, config) {
    const c = JSON.parse(config);
    const t = { c, origin, quic, channels: new Set(), closed: false };
    if (quic) {
        t.connection = new WebTransport(new URL(c.transport, origin));
        t.connection.closed.then(() => close(t), () => close(t));
    }
    return t;
}
/** Makes one cancellable channel, bounding its queued bytes and message count. */
function channel(t, limit) {
    if (t.closed) throw new Error("relay closed");
    const ch = { t, limit, queue: [], bytes: 0, ended: false, waiters: new Set() };
    ch.fail = () => {
        if (ch.failed) return;
        ch.failed = true; ch.queue = []; ch.bytes = 0;
        for (const cancel of ch.waiters) cancel(); ch.waiters.clear();
        ch.socket?.close();
        if (ch.reader) ch.reader.cancel().catch(() => {}).finally(() => ch.reader.releaseLock());
        if (ch.writer) ch.writer.abort().catch(() => {}).finally(() => ch.writer.releaseLock());
        t.channels.delete(ch);
    };
    t.channels.add(ch);
    return ch;
}
/** Races browser I/O against immediate channel cancellation. */
function wait(ch, promise) {
    return new Promise((resolve, reject) => {
        const cancel = () => reject(new Error("relay closed"));
        if (ch.failed) { Promise.resolve(promise).catch(() => {}); cancel(); return; }
        ch.waiters.add(cancel);
        Promise.resolve(promise).then(value => { ch.waiters.delete(cancel); resolve(value); },
            error => { ch.waiters.delete(cancel); reject(error); });
    });
}
/** Opens a QUIC stream or the named fallback socket; both return reliable byte chunks. */
export async function open(t, name) {
    const control = name === t.c.control;
    const ch = channel(t, !t.quic && control ? t.c.controlBytes + 4 : t.c.playBytes);
    if (t.quic) {
        await wait(ch, t.connection.ready);
        if (t.closed || ch.failed) throw new Error("relay closed");
        // Own late-created streams too when cancellation wins the stream-creation race.
        const creating = t.connection.createBidirectionalStream().then(stream => {
            ch.reader = stream.readable.getReader(); ch.writer = stream.writable.getWriter();
            if (ch.failed) {
                ch.reader.cancel().catch(() => {}).finally(() => ch.reader.releaseLock());
                ch.writer.abort().catch(() => {}).finally(() => ch.writer.releaseLock());
                throw new Error("relay closed");
            }
            return ch;
        });
        return wait(ch, creating);
    }
    const url = new URL(control ? t.c.controlPath : t.c.playPath, t.origin);
    url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
    const ws = ch.socket = new WebSocket(url, t.c.protocol);
    ws.binaryType = "arraybuffer";
    ws.onmessage = event => {
        if (!(event.data instanceof ArrayBuffer) || event.data.byteLength > ch.limit ||
            ch.bytes + event.data.byteLength > ch.limit || ch.queue.length === t.c.chunks) {
            close(t); return;
        }
        ch.queue.push(new Uint8Array(event.data)); ch.bytes += event.data.byteLength;
        ch.wake?.(); ch.wake = undefined;
    };
    ws.onclose = () => { ch.ended = true; ch.wake?.(); ch.wake = undefined; for (const cancel of ch.waiters) cancel(); ch.waiters.clear(); if (!ch.opened) ch.fail(); };
    ws.onerror = () => ch.fail();
    await wait(ch, new Promise(resolve => { ws.onopen = resolve; }));
    ch.opened = true;
    if (ws.protocol !== t.c.protocol) { ch.fail(); throw new Error("relay protocol"); }
    return ch;
}
/** Reads only on demand; checks chunk size before Rust makes its JS-to-wasm copy. */
export async function read(ch) {
    if (ch.failed) throw new Error("relay closed");
    if (ch.reader) {
        const { done, value } = await wait(ch, ch.reader.read());
        if (done) return null;
        if (!(value instanceof Uint8Array) || value.byteLength > ch.limit) {
            close(ch.t); throw new Error("relay chunk limit");
        }
        return value;
    }
    while (!ch.queue.length) {
        if (ch.ended) return null;
        try { await wait(ch, new Promise(resolve => { ch.wake = resolve; })); }
        catch (error) { if (!ch.ended || ch.failed) throw error; }
    }
    const bytes = ch.queue.shift(); ch.bytes -= bytes.byteLength;
    return bytes;
}
/** Re-arms every drain poll, clearing its timer when closure cancels a throttled send. */
async function drain(ch, threshold) {
    while (ch.socket.bufferedAmount > threshold) {
        if (ch.socket.readyState !== WebSocket.OPEN) throw new Error("relay closed");
        let timer;
        try { await wait(ch, new Promise(resolve => { timer = setTimeout(resolve, 4); })); }
        finally { clearTimeout(timer); }
    }
    if (ch.socket.readyState !== WebSocket.OPEN) throw new Error("relay closed");
}
/** Bounds each send and retains browser readiness until the write promise resolves. */
export function write(ch, bytes) {
    return ch.pendingWrite = (async () => {
    if (ch.failed || ch.shuttingDown) throw new Error("relay closed");
    if (bytes.byteLength > ch.limit) throw new Error("relay chunk limit");
    if (ch.writer) {
        await wait(ch, ch.writer.ready); await wait(ch, ch.writer.write(bytes));
    } else {
        await drain(ch, ch.limit - bytes.byteLength);
        if (ch.failed || ch.socket.readyState !== WebSocket.OPEN) throw new Error("relay closed");
        ch.socket.send(bytes);
    }
    })();
}
/** Flushes locally accepted reliable bytes; it does not acknowledge remote application. */
export async function flush(ch) { if (ch.socket) await drain(ch, 0); }
/** Finishes accepted writes before sending FIN; cancellation still uses closeChannel. */
export async function shutdown(ch) {
    if (ch.finished) return;
    ch.shuttingDown = true;
    if (ch.pendingWrite) await wait(ch, ch.pendingWrite);
    await flush(ch);
    if (ch.writer) {
        const writer = ch.writer;
        await wait(ch, writer.close());
        writer.releaseLock(); ch.writer = undefined;
        const reader = ch.reader;
        if (reader) { await wait(ch, reader.cancel()); reader.releaseLock(); ch.reader = undefined; }
    } else { ch.socket.close(); }
    ch.finished = true; ch.ended = true; ch.t.channels.delete(ch);
}
/** Releases both stream locks or the fallback socket without waiting for a drain. */
export function closeChannel(ch) { ch.fail(); }
/** Closes every channel of a candidate, including pending open/read/write promises. */
export function close(t) {
    if (t.closed) return;
    t.closed = true; for (const ch of t.channels) ch.fail(); t.connection?.close();
}
