// The worker thread behind bridge.cjs: its own connection to opal, one
// request and one reply per line, each reply posted back and signalled.
'use strict';

const net = require('net');
const {parentPort, workerData} = require('worker_threads');

const {port, replies, signal, token, worker} = workerData;
const socket = net.connect(port, '127.0.0.1');
socket.setNoDelay(true);
socket.write(`${JSON.stringify({role: 'transform', token, worker})}\n`);

let buffered = '';
socket.on('data', chunk => {
  buffered += chunk.toString('utf8');
  let newline;
  while ((newline = buffered.indexOf('\n')) >= 0) {
    const line = buffered.slice(0, newline);
    buffered = buffered.slice(newline + 1);
    replies.postMessage(JSON.parse(line));
    Atomics.store(signal, 0, 1);
    Atomics.notify(signal, 0);
  }
});
const fail = error => {
  replies.postMessage({error: `opal test: the transform channel failed: ${error && error.message}`});
  Atomics.store(signal, 0, 1);
  Atomics.notify(signal, 0);
};
socket.on('error', fail);
socket.on('close', () => fail(new Error('closed')));
parentPort.on('message', message => socket.write(`${JSON.stringify(message)}\n`));
