// A test worker: connects back to opal, then runs the files it is sent, one
// at a time. One JSON message per line each way; the first line is the
// token opal put in the environment, so opal knows which worker this is.
'use strict';

const net = require('net');
const {loadConfig} = require('./config.cjs');
const {runTestFile} = require('./runner.cjs');

const port = Number(process.env.OPAL_TEST_PORT);
const token = process.env.OPAL_TEST_TOKEN;
const worker = Number(process.env.OPAL_TEST_WORKER);
delete process.env.OPAL_TEST_PORT;
delete process.env.OPAL_TEST_TOKEN;
delete process.env.OPAL_TEST_WORKER;

// Between files nothing claims an uncaught error (each file's run installs
// its own handlers and puts these back after), and one must not take the
// worker down.
process.on('uncaughtException', error => {
  process.stderr.write(`opal test: uncaught error outside any test file: ${error && error.stack ? error.stack : error}\n`);
});
process.on('unhandledRejection', error => {
  process.stderr.write(`opal test: unhandled rejection outside any test file: ${error && error.stack ? error.stack : error}\n`);
});

const socket = net.connect(port, '127.0.0.1');
socket.setNoDelay(true);
const send = message => socket.write(`${JSON.stringify(message)}\n`);

socket.on('connect', () => send({token, worker}));
socket.on('error', () => process.exit(1));
socket.on('close', () => process.exit(0));

let buffered = '';
let queue = Promise.resolve();
socket.on('data', chunk => {
  buffered += chunk.toString('utf8');
  let newline;
  while ((newline = buffered.indexOf('\n')) >= 0) {
    const line = buffered.slice(0, newline);
    buffered = buffered.slice(newline + 1);
    if (!line) continue;
    const message = JSON.parse(line);
    queue = queue.then(() => handle(message));
  }
});

async function handle(message) {
  switch (message.type) {
    case 'config':
      try {
        send({type: 'config', config: await loadConfig(message)});
      } catch (error) {
        send({type: 'config', error: error && error.stack ? error.stack : String(error)});
      }
      break;
    case 'run': {
      let result;
      try {
        result = await runTestFile(message.path, message.context);
      } catch (error) {
        const stack = error && error.stack ? error.stack : String(error);
        result = {
          failureMessage: `  ● Test suite failed to run\n\n    ${stack}`,
          numFailingTests: 0,
          numPassingTests: 0,
          numPendingTests: 0,
          numTodoTests: 0,
          testExecError: {message: String(error && error.message), stack},
          testFilePath: message.path,
          testResults: [],
        };
      }
      send({type: 'result', result: toJSONSafe(result)});
      break;
    }
    case 'exit':
      socket.end(() => process.exit(0));
      break;
  }
}

// Results cross the wire as JSON; anything that doesn't survive that (a
// BigInt, a cycle) is replaced rather than allowed to fail the send.
function toJSONSafe(value) {
  const seen = new WeakSet();
  return JSON.parse(
    JSON.stringify(value, (_key, item) => {
      if (typeof item === 'bigint') return `${item}n`;
      if (item && typeof item === 'object') {
        if (seen.has(item)) return '[Circular]';
        seen.add(item);
      }
      return item;
    }),
  );
}
