// Qu Language extension — a minimal Language Server Protocol client.
// Qu editor plugin 0.2.2
//
// Speaks JSON-RPC 2.0 with `Content-Length` framing to `qu lsp` over the
// child's stdin/stdout. Deliberately free of any `vscode` import, so it can
// be exercised with plain `node` against a real `qu` binary (see
// `test/lsp_client_selftest.js` in the repository), and free of npm dependencies, so the
// extension stays what its README promises: a folder you can copy into
// `~/.vscode/extensions` with no build step. That is also why this is not
// `vscode-languageclient`: that package and its dependency closure are
// hundreds of files that `qu editors install` would have to embed and
// write out one by one.
'use strict';

const cp = require('child_process');
const { EventEmitter } = require('events');

const HEADER_END = Buffer.from('\r\n\r\n');

/**
 * Events:
 *   'notification' (method, params)   a server notification
 *   'exit' (code, signal)             the server process ended
 *   'error' (err)                     spawn failure or a protocol error
 *   'stderr' (text)                   what the server wrote to stderr
 */
class QuLspClient extends EventEmitter {
    /**
     * @param {string} command  the `qu` executable
     * @param {string[]} [args] defaults to ['lsp', '--stdio']
     * @param {object} [spawnOptions]
     */
    constructor(command, args, spawnOptions) {
        super();
        this.command = command;
        this.args = args || ['lsp', '--stdio'];
        this.spawnOptions = spawnOptions || {};
        this.child = null;
        this.nextId = 1;
        /** @type {Map<number, {resolve: Function, reject: Function}>} */
        this.pending = new Map();
        this.buffer = Buffer.alloc(0);
        this.capabilities = null;
        this.closed = false;
    }

    /**
     * Spawn the server and run the `initialize` / `initialized` handshake.
     * Resolves with the server's InitializeResult; rejects if the process
     * cannot be started or ends first (an older `qu` without `lsp` prints
     * "unknown command" and exits 1 — that is a rejection here, which the
     * extension turns into its `qu parse --json` fallback).
     */
    start(clientCapabilities) {
        return new Promise((resolve, reject) => {
            let settled = false;
            const fail = (err) => {
                if (!settled) {
                    settled = true;
                    reject(err);
                }
            };
            try {
                this.child = cp.spawn(this.command, this.args, Object.assign({ stdio: ['pipe', 'pipe', 'pipe'] }, this.spawnOptions));
            } catch (err) {
                fail(err);
                return;
            }
            this.child.on('error', (err) => {
                this._close(err);
                fail(err);
                this.emit('error', err);
            });
            this.child.on('exit', (code, signal) => {
                this._close(new Error(`qu lsp exited (code ${code}${signal ? `, signal ${signal}` : ''})`));
                fail(new Error(`qu lsp exited before initializing (code ${code})`));
                this.emit('exit', code, signal);
            });
            this.child.stdout.on('data', (chunk) => this._onData(chunk));
            this.child.stderr.on('data', (chunk) => this.emit('stderr', chunk.toString()));
            // Writing to a server that has already gone is reported through
            // 'exit', not as an unhandled stream error.
            this.child.stdin.on('error', () => {});

            this.request('initialize', {
                processId: process.pid,
                clientInfo: { name: 'vscode-qu' },
                rootUri: null,
                capabilities: clientCapabilities || {},
            }).then((result) => {
                this.capabilities = (result && result.capabilities) || {};
                this.notify('initialized', {});
                settled = true;
                resolve(result);
            }, fail);
        });
    }

    /** Send a request; resolves with `result`, rejects with the error reply. */
    request(method, params) {
        if (this.closed || !this.child) return Promise.reject(new Error('qu lsp is not running'));
        const id = this.nextId++;
        return new Promise((resolve, reject) => {
            this.pending.set(id, { resolve, reject });
            this._send({ jsonrpc: '2.0', id, method, params });
        });
    }

    notify(method, params) {
        if (this.closed || !this.child) return;
        this._send({ jsonrpc: '2.0', method, params });
    }

    /** `shutdown` then `exit`; kills the process if it does not go in time. */
    stop(timeoutMs) {
        if (this.closed || !this.child) return Promise.resolve();
        const child = this.child;
        return new Promise((resolve) => {
            const timer = setTimeout(() => {
                try {
                    child.kill();
                } catch (_) {
                    /* already gone */
                }
                resolve();
            }, timeoutMs || 2000);
            child.once('exit', () => {
                clearTimeout(timer);
                resolve();
            });
            this.request('shutdown', null).then(
                () => this.notify('exit', null),
                () => {}
            );
        });
    }

    _send(message) {
        const body = Buffer.from(JSON.stringify(message), 'utf8');
        // Content-Length counts BYTES of the UTF-8 body, not characters.
        const header = Buffer.from(`Content-Length: ${body.length}\r\n\r\n`, 'ascii');
        try {
            this.child.stdin.write(Buffer.concat([header, body]));
        } catch (_) {
            /* reported through 'exit' */
        }
    }

    _onData(chunk) {
        this.buffer = Buffer.concat([this.buffer, chunk]);
        for (;;) {
            const headerEnd = this.buffer.indexOf(HEADER_END);
            if (headerEnd < 0) return;
            const header = this.buffer.slice(0, headerEnd).toString('ascii');
            const m = /content-length:\s*(\d+)/i.exec(header);
            if (!m) {
                this.emit('error', new Error(`qu lsp sent a header without Content-Length: ${JSON.stringify(header)}`));
                this.buffer = Buffer.alloc(0);
                return;
            }
            const length = parseInt(m[1], 10);
            const bodyStart = headerEnd + HEADER_END.length;
            if (this.buffer.length < bodyStart + length) return; // wait for the rest
            const body = this.buffer.slice(bodyStart, bodyStart + length).toString('utf8');
            this.buffer = this.buffer.slice(bodyStart + length);
            let message;
            try {
                message = JSON.parse(body);
            } catch (err) {
                this.emit('error', new Error(`qu lsp sent invalid JSON: ${err.message}`));
                continue;
            }
            this._dispatch(message);
        }
    }

    _dispatch(message) {
        if (message.method !== undefined) {
            if (message.id !== undefined && message.id !== null) {
                // A server-to-client request. `qu lsp` sends none; answer
                // rather than leave one hanging if that ever changes.
                this._send({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'not supported by this client' } });
                return;
            }
            this.emit('notification', message.method, message.params);
            return;
        }
        const waiter = this.pending.get(message.id);
        if (!waiter) return;
        this.pending.delete(message.id);
        if (message.error) {
            const err = new Error(message.error.message || 'request failed');
            err.code = message.error.code;
            waiter.reject(err);
        } else {
            waiter.resolve(message.result);
        }
    }

    _close(err) {
        if (this.closed) return;
        this.closed = true;
        for (const waiter of this.pending.values()) waiter.reject(err);
        this.pending.clear();
    }
}

module.exports = { QuLspClient };
