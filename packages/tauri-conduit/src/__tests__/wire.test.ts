import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  readU8, writeU8,
  readU32LE, writeU32LE,
  readI64LE, writeI64LE,
  readF64LE, writeF64LE,
  readBool, writeBool,
  readString, writeString,
  readBytes, writeBytes,
} from '../codec/wire.js';

describe('wire codec', () => {
  it('u8 roundtrip', () => {
    const encoded = writeU8(42);
    const [val, consumed] = readU8(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 42);
    assert.equal(consumed, 1);
  });

  it('u32 LE roundtrip', () => {
    const encoded = writeU32LE(0xDEADBEEF);
    const [val, consumed] = readU32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 0xDEADBEEF);
    assert.equal(consumed, 4);
  });

  it('f64 LE roundtrip', () => {
    const encoded = writeF64LE(Math.PI);
    const [val, consumed] = readF64LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, Math.PI);
    assert.equal(consumed, 8);
  });

  it('bool roundtrip', () => {
    const encoded = writeBool(true);
    const [val, consumed] = readBool(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, true);
    assert.equal(consumed, 1);
  });

  it('string roundtrip', () => {
    const encoded = writeString('hello');
    const [val, consumed] = readString(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 'hello');
    assert.equal(consumed, encoded.byteLength);
  });

  it('bytes roundtrip', () => {
    const data = new Uint8Array([0xCA, 0xFE, 0xBA, 0xBE]);
    const encoded = writeBytes(data);
    const [val, consumed] = readBytes(encoded.buffer as ArrayBuffer, 0);
    assert.deepEqual(new Uint8Array(val), data);
    assert.equal(consumed, encoded.byteLength);
  });
});
