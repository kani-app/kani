'use strict';
const vm = require('vm');
const readline = require('readline');
const contexts = new Map();

process.on('uncaughtException', (err) => {
    process.stderr.write('v8_shim uncaughtException: ' + String(err) + '\n');
});

function makeContext() {
    const { webcrypto } = require('crypto');
    const sandbox = {
        crypto: webcrypto,
        TextEncoder, TextDecoder, URL, URLSearchParams,
        setTimeout, clearTimeout, setInterval, clearInterval, queueMicrotask,
        Promise, Uint8Array, Uint16Array, Uint32Array, Int8Array, Int16Array, Int32Array,
        Float32Array, Float64Array, DataView, ArrayBuffer, SharedArrayBuffer,
        JSON, Math, Date, RegExp, Error, Object, Array, String, Number, Boolean,
        Map, Set, WeakMap, WeakSet, Symbol, BigInt,
        parseInt, parseFloat, isNaN, isFinite, encodeURIComponent, decodeURIComponent,
        encodeURI, decodeURI, atob, btoa, console,
        Proxy, Reflect,
    };
    const ctx = vm.createContext(sandbox);
    sandbox.globalThis = sandbox;
    sandbox.global = sandbox;
    sandbox.window = sandbox;
    sandbox.self = sandbox;
    return ctx;
}

let queue = Promise.resolve();
function enqueue(fn) { queue = queue.then(fn).catch(() => {}); }

function respond(id, ok, value, error, callback) {
    const response = ok ? { id, ok, value } : { id, ok, error };
    process.stdout.write(JSON.stringify(response) + '\n', callback);
}

const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on('line', (line) => {
    enqueue(async () => {
        let req;
        try { req = JSON.parse(line.trim()); } catch { return; }
        const { id, action, name, script } = req;
        try {
            if (action === 'shutdown') {
                contexts.clear();
                respond(id, true, '', null, () => rl.close());

            } else if (action === 'exists') {
                respond(id, true, contexts.has(name) ? 'true' : 'false', null);

            } else if (action === 'create') {
                if (!contexts.has(name)) {
                    const ctx = makeContext();
                    const result = vm.runInContext(script, ctx, { filename: name });
                    if (result && typeof result.then === 'function') await result;
                    contexts.set(name, ctx);
                }
                respond(id, true, '', null);

            } else if (action === 'eval') {
                const ctx = contexts.get(name);
                if (!ctx) throw new Error(`V8 context '${name}' not found`);
                let result = vm.runInContext(script, ctx, { filename: `${name}:eval` });
                if (result && typeof result.then === 'function') result = await result;
                respond(id, true, String(result ?? ''), null);

            } else if (action === 'drop') {
                contexts.delete(name);
                respond(id, true, '', null);

            } else {
                throw new Error(`Unknown action: ${action}`);
            }
        } catch (e) {
            respond(id, false, null, e && e.kaniActionError ? e.kaniActionError : {
                code: 'action_error',
                message: String(e),
            });
        }
    });
});

process.stdout.write(JSON.stringify({ ready: true }) + '\n');
