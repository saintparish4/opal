// A test file's global scope: a fresh `vm` context holding Node's globals,
// the way jest-environment-node builds one (jest/packages/jest-environment-
// node/src/index.ts and jest-util's createProcessObject.ts).
//
// The context brings its own built-ins (Object, Array, Error, Promise...).
// Node's own globals (timers, URL, TextEncoder, fetch, structuredClone...)
// are added as lazy getters onto the host's, except Buffer, ArrayBuffer and
// Uint8Array, which are the host's outright so the bytes Node's modules hand
// back are the same kind the test checks for. `process` is a copy of the
// host's with its own event registry and its own environment, so what one
// file sets there no other file sees.
'use strict';

const vm = require('vm');
const {EventEmitter} = require('events');

const DENY = new Set(['GLOBAL', 'root', 'global', 'globalThis', 'Buffer', 'ArrayBuffer', 'Uint8Array']);
// Node 25+ storage proxies warn when read without --localstorage-file.
const STORAGE = new Set(['localStorage', 'sessionStorage']);

// Node's globals, by whether each is enumerable. jest-environment-node reads
// every global's descriptor here, but reading one materializes a lazy global:
// FormData's loads undici, 28 ms before a worker has run anything. Every
// Node global a fresh context lacks is configurable (Node 24), and a
// configurable one becomes a lazy getter below, so only enumerability is
// needed, and that check materializes nothing.
const nodeGlobals = new Map(
  Object.getOwnPropertyNames(globalThis)
    .filter(name => !DENY.has(name))
    .map(name => [name, Object.prototype.propertyIsEnumerable.call(globalThis, name)]),
);

// The worker's own environment, read once: nothing changes it between files,
// and reading process.env goes through Node's native accessor every time.
let hostEnv = null;

function createProcessEnv() {
  hostEnv ??= {...process.env};
  const proto = Object.getPrototypeOf(process.env);
  const real = Object.create(proto);
  const isWin32 = process.platform === 'win32';
  const lookup = {};
  const proxy = new Proxy(real, {
    deleteProperty(_target, key) {
      if (isWin32 && typeof key === 'string') {
        for (const name of Object.keys(real)) {
          if (name.toLowerCase() === key.toLowerCase()) {
            delete real[name];
            delete lookup[name.toLowerCase()];
          }
        }
        return true;
      }
      delete real[key];
      delete lookup[key];
      return true;
    },
    get(_target, key) {
      if (isWin32 && typeof key === 'string') return lookup[key in proto ? key : key.toLowerCase()];
      return real[key];
    },
    set(_target, key, value) {
      const strValue = `${value}`;
      if (typeof key === 'string') lookup[key.toLowerCase()] = strValue;
      real[key] = strValue;
      return true;
    },
  });
  for (const [key, value] of Object.entries(hostEnv)) {
    real[key] = value;
    lookup[key.toLowerCase()] = value;
  }
  return proxy;
}

const PROCESS_SKIP = new Set(['env', 'mainModule', '_events', '_eventsCount', '_maxListeners']);

// The host process's own properties, read once per worker, as each file's
// copy starts from them.
let processDescriptors = null;

function createProcessObject() {
  processDescriptors ??= Reflect.ownKeys(process)
    .filter(key => !PROCESS_SKIP.has(key))
    .map(key => [key, Object.getOwnPropertyDescriptor(process, key)]);
  const newProcess = Object.create(Object.getPrototypeOf(process));
  for (const [key, hostDescriptor] of processDescriptors) {
    const descriptor = {...hostDescriptor};
    if ('value' in descriptor && descriptor.value && typeof descriptor.value === 'object' && !Array.isArray(descriptor.value)) {
      const value = descriptor.value;
      // Plain records (versions, release, config, features) are copied so a
      // test can change them without reaching the host.
      if (Object.getPrototypeOf(value) === Object.prototype || Object.getPrototypeOf(value) === null) {
        descriptor.value = {...value};
      }
    }
    try {
      // Configurable on the copy, whatever it is on the host, so the
      // adjustments below and a test's own can be made.
      Object.defineProperty(newProcess, key, {...descriptor, configurable: true});
    } catch {
      // A few properties can't be redefined; the prototype still has them.
    }
  }
  try {
    newProcess[Symbol.toStringTag] = 'process';
  } catch {}
  EventEmitter.call(newProcess);
  newProcess.env = createProcessEnv();
  newProcess.send = () => true;
  Object.defineProperty(newProcess, 'domain', {get: () => process.domain});
  // Jest's runtime can't require ESM from CJS, and says so.
  if (newProcess.features && 'require_module' in newProcess.features) {
    Object.defineProperty(newProcess, 'features', {
      configurable: true,
      enumerable: true,
      value: {...newProcess.features, require_module: false},
      writable: true,
    });
  }
  return newProcess;
}

function createEnvironment(config) {
  const context = vm.createContext({});
  const global = vm.runInContext('this', Object.assign(context, config.testEnvironmentOptions || {}));
  const contextGlobals = new Set(Object.getOwnPropertyNames(global));
  for (const [name, enumerable] of nodeGlobals) {
    if (contextGlobals.has(name)) continue;
    Object.defineProperty(global, name, {
      configurable: true,
      enumerable,
      get() {
        const value = globalThis[name];
        if (!STORAGE.has(name)) {
          Object.defineProperty(global, name, {configurable: true, enumerable, value, writable: true});
        }
        return value;
      },
      set(value) {
        Object.defineProperty(global, name, {configurable: true, enumerable, value, writable: true});
      },
    });
  }
  global.global = global;
  global.Buffer = Buffer;
  global.ArrayBuffer = ArrayBuffer;
  // TextEncoder and friends hand back the host's Uint8Array; the test has to
  // see the same one.
  global.Uint8Array = Uint8Array;
  global.process = createProcessObject();
  global.Error.stackTraceLimit = 100;
  return {context, global};
}

module.exports = {createEnvironment};
