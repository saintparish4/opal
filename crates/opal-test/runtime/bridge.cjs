// A synchronous call to opal from a worker's main thread, for a transform
// needed in the middle of a `require`.
//
// A worker thread owns a second connection to opal. The main thread hands it
// a request and sleeps on Atomics.wait until the thread has the reply, then
// takes the reply with receiveMessageOnPort, which doesn't need the event
// loop. synckit does the same; it works alike on every platform.
'use strict';

const path = require('path');
const {MessageChannel, Worker, receiveMessageOnPort} = require('worker_threads');

// The thread starts on the first request, so a run that transforms nothing
// never pays for it.
class Bridge {
  constructor(options) {
    this.options = options;
    this.thread = null;
  }

  start() {
    const {port, token, worker} = this.options;
    this.signal = new Int32Array(new SharedArrayBuffer(4));
    const {port1, port2} = new MessageChannel();
    this.replies = port1;
    this.thread = new Worker(path.join(__dirname, 'bridge-thread.cjs'), {
      transferList: [port2],
      workerData: {port, replies: port2, signal: this.signal, token, worker},
    });
    this.thread.unref();
  }

  request(message) {
    if (!this.thread) this.start();
    Atomics.store(this.signal, 0, 0);
    this.thread.postMessage(message);
    Atomics.wait(this.signal, 0, 0);
    const received = receiveMessageOnPort(this.replies);
    if (!received) throw new Error('opal test: the transform channel closed without a reply');
    const reply = received.message;
    return typeof reply === 'string' ? JSON.parse(reply) : reply;
  }
}

module.exports = {Bridge};
