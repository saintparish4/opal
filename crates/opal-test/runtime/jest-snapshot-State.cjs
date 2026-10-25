// Ported from jest/packages/jest-snapshot/src/State.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, 'default', {enumerable: true, get: () => SnapshotState});
const _m0 = require("fs");
const _m1 = require("./snapfile-dialect.cjs");
const _m2 = require("./support.cjs");
const _m3 = require("./inline-snapshots.cjs");
const _m4 = require("./jest-snapshot-utils.cjs");

/**
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */



                                        


                                                            

                                    
                                                      
                                        
                            
                                          
                           
  

                                    
                            
                                 
                             
                        
                                   
                             
                         
                                 
  

                              
                          
                         
                             
                       
                         
  

                   
                   
                 
  

                       
                
                  
                    
                  
  

// The dirty flag is monotonic, so undoing it per test needs proof that no
// other test wrote in between: the value it had before this test's first
// write, and how many of the writes since then were this test's own.
                      
                       
                    
                           
  

// Everything a single test contributed since its current attempt began, so a
// retry can undo exactly that much and leave the other tests' work alone.
                      
                           
                                            
                         
                                                 
                                       
                                    
  

class SnapshotState {
          _counters                     ;
          _dirty         ;
                   _updateSnapshot                            ;
                   _snapshotData              ;
                   _snapshotPath        ;
          _inlineSnapshots                       ;
          _attemptRecordsByTest                                ;
          _writeCount        ;
                   _uncheckedKeys             ;
                   _prettierPath               ;
                   _rootDir        ;

           snapshotFormat                ;

  added        ;
  expand         ;
  matched        ;
  unmatched        ;
  updated        ;

  constructor(snapshotPath        , options                      ) {
    this._snapshotPath = snapshotPath;
    const {data, dirty} = _m1.getSnapshotData(
      this._snapshotPath,
      options.updateSnapshot,
    );
    this._snapshotData = data;
    this._dirty = dirty;
    this._prettierPath = options.prettierPath ?? null;
    this._inlineSnapshots = [];
    this._attemptRecordsByTest = new WeakMap();
    this._writeCount = 0;
    this._uncheckedKeys = new Set(Object.keys(this._snapshotData));
    this._counters = new Map();
    this.expand = options.expand || false;
    this.added = 0;
    this.matched = 0;
    this.unmatched = 0;
    this._updateSnapshot = options.updateSnapshot;
    this.updated = 0;
    this.snapshotFormat = options.snapshotFormat;
    this._rootDir = options.rootDir;
  }

  /** @internal */
  get snapshotPath()         {
    return this._snapshotPath;
  }

  markSnapshotsAsCheckedForTest(testName        )       {
    for (const uncheckedKey of this._uncheckedKeys) {
      const keyTestName = _m1.keyToTestName(uncheckedKey);
      // A hint is joined onto the test name with ': ' before the key is built,
      // so the recovered name of a hinted snapshot is longer than the name the
      // runner reports for the test that took it.
      if (keyTestName === testName || keyTestName.startsWith(`${testName}: `)) {
        this._uncheckedKeys.delete(uncheckedKey);
      }
    }
  }

          _addSnapshot(
    key        ,
    receivedSerialized        ,
    options   
                    
                        
                            
     ,
  )       {
    const record = this._recordFor(options.testIdentity);
    if (record !== undefined) {
      record.writes ??= {
        dirtyBefore: this._dirty,
        ownWrites: 0,
        writeCountBefore: this._writeCount,
      };
      record.writes.ownWrites++;
    }
    this._writeCount++;
    this._dirty = true;
    if (options.isInline) {
      // eslint-disable-next-line unicorn/error-message
      const error = options.error || new Error();
      const lines = _m2.getStackTraceLines(
        _m4.removeLinesBeforeExternalMatcherTrap(error.stack || ''),
      );
      const frame = _m2.getTopFrame(lines);
      if (!frame) {
        throw new Error(
          "Jest: Couldn't infer stack frame for inline snapshot.",
        );
      }
      const inlineSnapshot = {
        frame,
        snapshot: receivedSerialized,
      };
      this._inlineSnapshots.push(inlineSnapshot);
      record?.inlineSnapshots.add(inlineSnapshot);
    } else {
      // A retried attempt must not leave its writes behind, or the next attempt
      // sees the key as pre-existing and reports it as matched, not written.
      if (record !== undefined && !record.fileSnapshots.has(key)) {
        record.fileSnapshots.set(key, this._snapshotData[key]);
      }
      this._snapshotData[key] = receivedSerialized;
    }
  }

          _recordFor(testIdentity         )                            {
    if (testIdentity === undefined) {
      return undefined;
    }
    let record = this._attemptRecordsByTest.get(testIdentity);
    if (record === undefined) {
      record = {
        checkedKeys: new Set(),
        counters: new Map(),
        counts: {added: 0, matched: 0, unmatched: 0, updated: 0},
        fileSnapshots: new Map(),
        inlineSnapshots: new Set(),
        writes: undefined,
      };
      this._attemptRecordsByTest.set(testIdentity, record);
    }
    return record;
  }

  // `match` marks a key as checked so it is not reported obsolete. A key only
  // reached on a discarded attempt has to go back to being unchecked.
          _markKeyChecked(key        , testIdentity         )       {
    if (this._uncheckedKeys.delete(key)) {
      this._recordFor(testIdentity)?.checkedKeys.add(key);
    }
  }

  // Counters number keys per full test name, and full names can collide
  // across tests. Undoing an increment could double-undo a collision, so the
  // record keeps each counter's value from before the test first touched it.
          _bumpCounter(testName        , testIdentity         )         {
    const record = this._recordFor(testIdentity);
    if (record !== undefined && !record.counters.has(testName)) {
      record.counters.set(testName, this._counters.get(testName));
    }
    const count = (this._counters.get(testName) ?? 0) + 1;
    this._counters.set(testName, count);
    return count;
  }

  clear(testIdentity         )       {
    if (testIdentity === undefined) {
      this._counters = new Map();
      // TODO(jest next major): require `testIdentity` and drop this branch.
      // Unlike the per-test path below it rolls back neither file snapshot
      // data nor unchecked keys, so a snapshot added before the reset is
      // reported as matched and one it checked is never reported obsolete.
      this._inlineSnapshots = [];
      this._attemptRecordsByTest = new WeakMap();
      this.added = 0;
      this.matched = 0;
      this.unmatched = 0;
      this.updated = 0;
      return;
    }

    const record = this._attemptRecordsByTest.get(testIdentity);
    if (record === undefined) {
      return;
    }
    this._attemptRecordsByTest.delete(testIdentity);

    this._inlineSnapshots = this._inlineSnapshots.filter(
      snapshot => !record.inlineSnapshots.has(snapshot),
    );
    this.added -= record.counts.added;
    this.matched -= record.counts.matched;
    this.unmatched -= record.counts.unmatched;
    this.updated -= record.counts.updated;
    for (const [key, previous] of record.fileSnapshots) {
      if (previous === undefined) {
        delete this._snapshotData[key];
      } else {
        this._snapshotData[key] = previous;
      }
    }
    for (const key of record.checkedKeys) {
      this._uncheckedKeys.add(key);
    }
    for (const [testName, previous] of record.counters) {
      if (previous === undefined) {
        this._counters.delete(testName);
      } else {
        this._counters.set(testName, previous);
      }
    }
    // The dirty flag can only be undone when every write since this test's
    // first was its own; a foreign write in between keeps the flag earned.
    const writes = record.writes;
    if (
      writes !== undefined &&
      this._writeCount - writes.writeCountBefore === writes.ownWrites
    ) {
      this._dirty = writes.dirtyBefore;
      this._writeCount = writes.writeCountBefore;
    }
  }

          _incrementSnapshotCount(
    status                      ,
    testIdentity         ,
  )       {
    this[status]++;
    const record = this._recordFor(testIdentity);
    if (record !== undefined) {
      record.counts[status]++;
    }
  }

  save()             {
    const hasExternalSnapshots = Object.keys(this._snapshotData).length;
    const hasInlineSnapshots = this._inlineSnapshots.length;
    const isEmpty = !hasExternalSnapshots && !hasInlineSnapshots;

    const status             = {
      deleted: false,
      saved: false,
    };

    if ((this._dirty || this._uncheckedKeys.size > 0) && !isEmpty) {
      if (hasExternalSnapshots) {
        _m1.saveSnapshotFile(this._snapshotData, this._snapshotPath);
      }
      if (hasInlineSnapshots) {
        _m3.saveInlineSnapshots(
          this._inlineSnapshots,
          this._rootDir,
          this._prettierPath,
        );
      }
      status.saved = true;
    } else if (!hasExternalSnapshots && _m0.existsSync(this._snapshotPath)) {
      if (this._updateSnapshot === 'all') {
        _m0.unlinkSync(this._snapshotPath);
      }
      status.deleted = true;
    }

    return status;
  }

  getUncheckedCount()         {
    return this._uncheckedKeys.size || 0;
  }

  getUncheckedKeys()                {
    return [...this._uncheckedKeys];
  }

  removeUncheckedKeys()       {
    if (this._updateSnapshot === 'all' && this._uncheckedKeys.size > 0) {
      this._dirty = true;
      for (const key of this._uncheckedKeys) delete this._snapshotData[key];
      this._uncheckedKeys.clear();
    }
  }

  match({
    testName,
    testIdentity,
    received,
    key,
    inlineSnapshot,
    isInline,
    error,
    testFailing = false,
  }                      )                        {
    const count = this._bumpCounter(testName, testIdentity);

    if (!key) {
      key = _m1.testNameToKey(testName, count);
    }

    // Do not mark the snapshot as "checked" if the snapshot is inline and
    // there's an external snapshot. This way the external snapshot can be
    // removed with `--updateSnapshot`.
    if (!(isInline && this._snapshotData[key] !== undefined)) {
      this._markKeyChecked(key, testIdentity);
    }

    const receivedSerialized = _m4.addExtraLineBreaks(
      _m4.serialize(received, undefined, this.snapshotFormat),
    );
    const expected = isInline ? inlineSnapshot : this._snapshotData[key];
    const pass = expected === receivedSerialized;
    const hasSnapshot = expected !== undefined;
    const snapshotIsPersisted = isInline || _m0.existsSync(this._snapshotPath);

    if (pass && !isInline) {
      // Executing a snapshot file as JavaScript and writing the strings back
      // when other snapshots have changed loses the proper escaping for some
      // characters. Since we check every snapshot in every test, use the newly
      // generated formatted string.
      // Note that this is only relevant when a snapshot is added and the dirty
      // flag is set.
      this._snapshotData[key] = receivedSerialized;
    }

    // In pure matching only runs, return the match result while skipping any updates
    // reports.
    if (testFailing) {
      if (hasSnapshot && !isInline) {
        // Retain current snapshot values.
        this._addSnapshot(key, expected, {error, isInline, testIdentity});
      }
      return {
        actual: _m4.removeExtraLineBreaks(receivedSerialized),
        count,
        expected:
          expected === undefined ? undefined : _m4.removeExtraLineBreaks(expected),
        key,
        pass,
      };
    }

    // These are the conditions on when to write snapshots:
    //  * There's no snapshot file in a non-CI environment.
    //  * There is a snapshot file and we decided to update the snapshot.
    //  * There is a snapshot file, but it doesn't have this snapshot.
    // These are the conditions on when not to write snapshots:
    //  * The update flag is set to 'none'.
    //  * There's no snapshot file or a file without this snapshot on a CI environment.
    if (
      (hasSnapshot && this._updateSnapshot === 'all') ||
      ((!hasSnapshot || !snapshotIsPersisted) &&
        (this._updateSnapshot === 'new' || this._updateSnapshot === 'all'))
    ) {
      if (this._updateSnapshot === 'all') {
        if (pass) {
          this._incrementSnapshotCount('matched', testIdentity);
        } else {
          if (hasSnapshot) {
            this._incrementSnapshotCount('updated', testIdentity);
          } else {
            this._incrementSnapshotCount('added', testIdentity);
          }
          this._addSnapshot(key, receivedSerialized, {
            error,
            isInline,
            testIdentity,
          });
        }
      } else {
        this._addSnapshot(key, receivedSerialized, {
          error,
          isInline,
          testIdentity,
        });
        this._incrementSnapshotCount('added', testIdentity);
      }

      return {
        actual: '',
        count,
        expected: '',
        key,
        pass: true,
      };
    } else {
      if (pass) {
        this._incrementSnapshotCount('matched', testIdentity);
        return {
          actual: '',
          count,
          expected: '',
          key,
          pass: true,
        };
      } else {
        this._incrementSnapshotCount('unmatched', testIdentity);
        return {
          actual: _m4.removeExtraLineBreaks(receivedSerialized),
          count,
          expected:
            expected === undefined
              ? undefined
              : _m4.removeExtraLineBreaks(expected),
          key,
          pass: false,
        };
      }
    }
  }

  fail(
    testName        ,
    _received         ,
    key         ,
    testIdentity         ,
  )         {
    const count = this._bumpCounter(testName, testIdentity);

    if (!key) {
      key = _m1.testNameToKey(testName, count);
    }

    this._markKeyChecked(key, testIdentity);
    this._incrementSnapshotCount('unmatched', testIdentity);
    return key;
  }
}
