// Ported from jest/packages/expect/src/asymmetricMatchers.ts at Jest v30.5.2 by
// crates/opal-test/port/port.mjs: types stripped, ES modules rewritten to
// CommonJS, nothing else changed. Do not edit by hand; edit the port.
//
// Copyright (c) Meta Platforms, Inc. and affiliates. MIT license, in
// compat/test-runners/LICENSES/jest.txt.
'use strict';
Object.defineProperty(exports, "hasProperty", {enumerable: true, get: () => hasProperty});
Object.defineProperty(exports, "AsymmetricMatcher", {enumerable: true, get: () => AsymmetricMatcher});
Object.defineProperty(exports, "any", {enumerable: true, get: () => any});
Object.defineProperty(exports, "anything", {enumerable: true, get: () => anything});
Object.defineProperty(exports, "arrayContaining", {enumerable: true, get: () => arrayContaining});
Object.defineProperty(exports, "arrayNotContaining", {enumerable: true, get: () => arrayNotContaining});
Object.defineProperty(exports, "arrayOf", {enumerable: true, get: () => arrayOf});
Object.defineProperty(exports, "notArrayOf", {enumerable: true, get: () => notArrayOf});
Object.defineProperty(exports, "objectContaining", {enumerable: true, get: () => objectContaining});
Object.defineProperty(exports, "objectNotContaining", {enumerable: true, get: () => objectNotContaining});
Object.defineProperty(exports, "stringContaining", {enumerable: true, get: () => stringContaining});
Object.defineProperty(exports, "stringNotContaining", {enumerable: true, get: () => stringNotContaining});
Object.defineProperty(exports, "stringMatching", {enumerable: true, get: () => stringMatching});
Object.defineProperty(exports, "stringNotMatching", {enumerable: true, get: () => stringNotMatching});
Object.defineProperty(exports, "closeTo", {enumerable: true, get: () => closeTo});
Object.defineProperty(exports, "notCloseTo", {enumerable: true, get: () => notCloseTo});
const _m0 = require("./jest-expect-utils.cjs");
const _m1 = require("./jest-matcher-utils.cjs");
const _m2 = require("./support.cjs");
const _m3 = require("./jest-expect-jestMatchersObject.cjs");





const functionToString = Function.prototype.toString;
function fnNameFor(func) {
    if (func.name) {
        return func.name;
    }
    const matches = functionToString.call(func).match(/^(?:async)?\s*function\s*\*?\s*([\w$]+)\s*\(/);
    return matches ? matches[1] : '<anonymous>';
}
const utils = Object.freeze({
    ..._m1,
    iterableEquality: _m0.iterableEquality,
    subsetEquality: _m0.subsetEquality
});
function hasProperty(obj, property) {
    if (!obj) {
        return false;
    }
    if (Object.prototype.hasOwnProperty.call(obj, property)) {
        return true;
    }
    return hasProperty(Object.getPrototypeOf(obj), property);
}
class AsymmetricMatcher {
    sample;
    inverse;
    $$typeof = Symbol.for('jest.asymmetricMatcher');
    constructor(sample, inverse = false){
        this.sample = sample;
        this.inverse = inverse;
    }
    getMatcherContext() {
        return {
            customTesters: _m3.getCustomEqualityTesters(),
            dontThrow: ()=>{},
            ..._m3.getState(),
            equals: _m0.equals,
            isNot: this.inverse,
            utils
        };
    }
}
class Any extends AsymmetricMatcher {
    constructor(sample){
        if (sample === undefined) {
            throw new TypeError('any() expects to be passed a constructor function. ' + 'Please pass one or use anything() to match any object.');
        }
        super(sample);
    }
    asymmetricMatch(other) {
        if (this.sample === String) {
            return typeof other === 'string' || other instanceof String;
        }
        if (this.sample === Number) {
            return typeof other === 'number' || other instanceof Number;
        }
        if (this.sample === Function) {
            return typeof other === 'function' || other instanceof Function;
        }
        if (this.sample === Boolean) {
            return typeof other === 'boolean' || other instanceof Boolean;
        }
        if (this.sample === BigInt) {
            return typeof other === 'bigint' || other instanceof BigInt;
        }
        if (this.sample === Symbol) {
            return typeof other === 'symbol' || other instanceof Symbol;
        }
        if (this.sample === Object) {
            return typeof other === 'object';
        }
        if (this.sample === Array) {
            return Array.isArray(other);
        }
        return other instanceof this.sample;
    }
    toString() {
        return 'Any';
    }
    getExpectedType() {
        if (this.sample === String) {
            return 'string';
        }
        if (this.sample === Number) {
            return 'number';
        }
        if (this.sample === Function) {
            return 'function';
        }
        if (this.sample === Object) {
            return 'object';
        }
        if (this.sample === Boolean) {
            return 'boolean';
        }
        if (this.sample === Array) {
            return 'array';
        }
        return fnNameFor(this.sample);
    }
    toAsymmetricMatcher() {
        return `Any<${fnNameFor(this.sample)}>`;
    }
}
class Anything extends AsymmetricMatcher {
    asymmetricMatch(other) {
        return other != null;
    }
    toString() {
        return 'Anything';
    }
    toAsymmetricMatcher() {
        return 'Anything';
    }
}
class ArrayContaining extends AsymmetricMatcher {
    constructor(sample, inverse = false){
        super(sample, inverse);
    }
    asymmetricMatch(other) {
        if (!Array.isArray(this.sample)) {
            throw new TypeError(`You must provide an array to ${this.toString()}, not '${typeof this.sample}'.`);
        }
        const matcherContext = this.getMatcherContext();
        const result = this.sample.length === 0 || Array.isArray(other) && this.sample.every((item)=>other.some((another)=>_m0.equals(item, another, matcherContext.customTesters)));
        return this.inverse ? !result : result;
    }
    toString() {
        return `Array${this.inverse ? 'Not' : ''}Containing`;
    }
    getExpectedType() {
        return 'array';
    }
}
class ArrayOf extends AsymmetricMatcher {
    asymmetricMatch(other) {
        const matcherContext = this.getMatcherContext();
        const result = Array.isArray(other) && other.every((item)=>_m0.equals(this.sample, item, matcherContext.customTesters));
        return this.inverse ? !result : result;
    }
    toString() {
        return `${this.inverse ? 'Not' : ''}ArrayOf`;
    }
    getExpectedType() {
        return 'array';
    }
}
class ObjectContaining extends AsymmetricMatcher {
    constructor(sample, inverse = false){
        super(sample, inverse);
    }
    asymmetricMatch(other) {
        if (typeof this.sample !== 'object') {
            throw new TypeError(`You must provide an object to ${this.toString()}, not '${typeof this.sample}'.`);
        }
        if (typeof other !== 'object' || Array.isArray(other)) {
            return false;
        }
        let result = true;
        const matcherContext = this.getMatcherContext();
        const objectKeys = _m0.getObjectKeys(this.sample);
        for (const key of objectKeys){
            if (!hasProperty(other, key) || !_m0.equals(this.sample[key], other[key], matcherContext.customTesters)) {
                result = false;
                break;
            }
        }
        return this.inverse ? !result : result;
    }
    toString() {
        return `Object${this.inverse ? 'Not' : ''}Containing`;
    }
    getExpectedType() {
        return 'object';
    }
}
class StringContaining extends AsymmetricMatcher {
    constructor(sample, inverse = false){
        if (!_m0.isA('String', sample)) {
            throw new Error('Expected is not a string');
        }
        super(sample, inverse);
    }
    asymmetricMatch(other) {
        const result = _m0.isA('String', other) && other.includes(this.sample);
        return this.inverse ? !result : result;
    }
    toString() {
        return `String${this.inverse ? 'Not' : ''}Containing`;
    }
    getExpectedType() {
        return 'string';
    }
}
class StringMatching extends AsymmetricMatcher {
    constructor(sample, inverse = false){
        if (!_m0.isA('String', sample) && !_m0.isA('RegExp', sample)) {
            throw new Error('Expected is not a String or a RegExp');
        }
        super(new RegExp(sample), inverse);
    }
    asymmetricMatch(other) {
        const result = _m0.isA('String', other) && this.sample.test(other);
        return this.inverse ? !result : result;
    }
    toString() {
        return `String${this.inverse ? 'Not' : ''}Matching`;
    }
    getExpectedType() {
        return 'string';
    }
}
class CloseTo extends AsymmetricMatcher {
    precision;
    constructor(sample, precision = 2, inverse = false){
        if (!_m0.isA('Number', sample)) {
            throw new Error('Expected is not a Number');
        }
        if (!_m0.isA('Number', precision)) {
            throw new Error('Precision is not a Number');
        }
        super(sample);
        this.inverse = inverse;
        this.precision = precision;
    }
    asymmetricMatch(other) {
        if (!_m0.isA('Number', other)) {
            return false;
        }
        let result;
        if (other === Number.POSITIVE_INFINITY && this.sample === Number.POSITIVE_INFINITY) {
            result = true;
        } else if (other === Number.NEGATIVE_INFINITY && this.sample === Number.NEGATIVE_INFINITY) {
            result = true;
        } else {
            result = Math.abs(this.sample - other) < Math.pow(10, -this.precision) / 2;
        }
        return this.inverse ? !result : result;
    }
    toString() {
        return `Number${this.inverse ? 'Not' : ''}CloseTo`;
    }
    getExpectedType() {
        return 'number';
    }
    toAsymmetricMatcher() {
        return [
            this.toString(),
            this.sample,
            `(${_m2.pluralize('digit', this.precision)})`
        ].join(' ');
    }
}
const any = (expectedObject)=>new Any(expectedObject);
const anything = ()=>new Anything();
const arrayContaining = (sample)=>new ArrayContaining(sample);
const arrayNotContaining = (sample)=>new ArrayContaining(sample, true);
const arrayOf = (sample)=>new ArrayOf(sample);
const notArrayOf = (sample)=>new ArrayOf(sample, true);
const objectContaining = (sample)=>new ObjectContaining(sample);
const objectNotContaining = (sample)=>new ObjectContaining(sample, true);
const stringContaining = (expected)=>new StringContaining(expected);
const stringNotContaining = (expected)=>new StringContaining(expected, true);
const stringMatching = (expected)=>new StringMatching(expected);
const stringNotMatching = (expected)=>new StringMatching(expected, true);
const closeTo = (expected, precision)=>new CloseTo(expected, precision);
const notCloseTo = (expected, precision)=>new CloseTo(expected, precision, true);
