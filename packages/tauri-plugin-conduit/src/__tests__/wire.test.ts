import { describe, it, test } from 'node:test';
import assert from 'node:assert/strict';
import {
  readU8, writeU8,
  readU16LE, writeU16LE,
  readU32LE, writeU32LE,
  readU64LE, writeU64LE,
  readI8, writeI8,
  readI16LE, writeI16LE,
  readI32LE, writeI32LE,
  readI64LE, writeI64LE,
  readF32LE, writeF32LE,
  readF64LE, writeF64LE,
  readBool, writeBool,
  readString, writeString,
  readBytes, writeBytes,
  parseDrainBlob,
} from '../codec/wire.js';

describe('wire codec', () => {
  it('u8 roundtrip', () => {
    const encoded = writeU8(42);
    const [val, consumed] = readU8(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 42);
    assert.equal(consumed, 1);
  });

  it('u16 LE roundtrip', () => {
    const encoded = writeU16LE(0xBEEF);
    const [val, consumed] = readU16LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 0xBEEF);
    assert.equal(consumed, 2);
  });

  it('u32 LE roundtrip', () => {
    const encoded = writeU32LE(0xDEADBEEF);
    const [val, consumed] = readU32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 0xDEADBEEF);
    assert.equal(consumed, 4);
  });

  it('u64 LE roundtrip (bigint)', () => {
    const encoded = writeU64LE(0xDEADBEEFCAFEBABEn);
    const [val, consumed] = readU64LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 0xDEADBEEFCAFEBABEn);
    assert.equal(consumed, 8);
  });

  it('i8 roundtrip', () => {
    const encoded = writeI8(-42);
    const [val, consumed] = readI8(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, -42);
    assert.equal(consumed, 1);
  });

  it('i16 LE roundtrip', () => {
    const encoded = writeI16LE(-12345);
    const [val, consumed] = readI16LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, -12345);
    assert.equal(consumed, 2);
  });

  it('i32 LE roundtrip', () => {
    const encoded = writeI32LE(-123456789);
    const [val, consumed] = readI32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, -123456789);
    assert.equal(consumed, 4);
  });

  it('f32 LE roundtrip', () => {
    const encoded = writeF32LE(3.140000104904175);
    const [val, consumed] = readF32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 3.140000104904175);
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

describe('additional wire codec tests', () => {
  it('non-zero offset read (two sequential values)', () => {
    // Write two u32 values back-to-back, read second at offset 4
    const a = writeU32LE(0x11223344);
    const b = writeU32LE(0xAABBCCDD);
    const combined = new Uint8Array(8);
    combined.set(a, 0);
    combined.set(b, 4);
    const [val, consumed] = readU32LE(combined.buffer as ArrayBuffer, 4);
    assert.equal(val, 0xAABBCCDD);
    assert.equal(consumed, 4);
  });

  it('u32 LE edge value 0xFFFFFFFF', () => {
    const encoded = writeU32LE(0xFFFFFFFF);
    const [val, consumed] = readU32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, 0xFFFFFFFF);
    assert.equal(consumed, 4);
  });

  it('i32 LE edge value -1', () => {
    const encoded = writeI32LE(-1);
    const [val, consumed] = readI32LE(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, -1);
    assert.equal(consumed, 4);
  });

  it('bool false roundtrip', () => {
    const encoded = writeBool(false);
    const [val, consumed] = readBool(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, false);
    assert.equal(consumed, 1);
  });

  it('empty string roundtrip', () => {
    const encoded = writeString('');
    const [val, consumed] = readString(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, '');
    assert.equal(consumed, 4); // 4-byte length prefix, 0 bytes payload
  });

  it('unicode string roundtrip', () => {
    const str = 'hello 世界 🌍';
    const encoded = writeString(str);
    const [val, consumed] = readString(encoded.buffer as ArrayBuffer, 0);
    assert.equal(val, str);
    assert.equal(consumed, encoded.byteLength);
  });
});

test('parseDrainBlob empty buffer', () => {
  assert.deepStrictEqual(parseDrainBlob(new ArrayBuffer(0)), []);
});

test('parseDrainBlob single frame', () => {
  const buf = new ArrayBuffer(4 + 4 + 3);
  const view = new DataView(buf);
  view.setUint32(0, 1, true); // count = 1
  view.setUint32(4, 3, true); // len = 3
  new Uint8Array(buf, 8, 3).set([1, 2, 3]);
  const frames = parseDrainBlob(buf);
  assert.strictEqual(frames.length, 1);
  assert.deepStrictEqual([...frames[0]], [1, 2, 3]);
});

test('parseDrainBlob multiple frames', () => {
  const buf = new ArrayBuffer(4 + 4 + 2 + 4 + 1);
  const view = new DataView(buf);
  view.setUint32(0, 2, true); // count = 2
  view.setUint32(4, 2, true); // frame 1 len
  new Uint8Array(buf, 8, 2).set([0xAA, 0xBB]);
  view.setUint32(10, 1, true); // frame 2 len
  new Uint8Array(buf, 14, 1).set([0xCC]);
  const frames = parseDrainBlob(buf);
  assert.strictEqual(frames.length, 2);
  assert.deepStrictEqual([...frames[0]], [0xAA, 0xBB]);
  assert.deepStrictEqual([...frames[1]], [0xCC]);
});

test('parseDrainBlob truncated buffer throws RangeError', () => {
  const buf = new ArrayBuffer(4); // count header only, no frames
  new DataView(buf).setUint32(0, 5, true); // claims 5 frames
  assert.throws(() => parseDrainBlob(buf), RangeError);
});

test('parseDrainBlob zero-length frame', () => {
  // frame count = 1, len = 0 (empty payload)
  const buf = new ArrayBuffer(4 + 4);
  const view = new DataView(buf);
  view.setUint32(0, 1, true); // count = 1
  view.setUint32(4, 0, true); // len = 0
  const frames = parseDrainBlob(buf);
  assert.strictEqual(frames.length, 1);
  assert.strictEqual(frames[0].byteLength, 0);
});

test('readU8 throws on out-of-bounds', () => {
  assert.throws(() => readU8(new ArrayBuffer(0), 0), RangeError);
});

test('readU32LE throws on out-of-bounds', () => {
  assert.throws(() => readU32LE(new ArrayBuffer(2), 0), RangeError);
});

test('readBool non-zero values are truthy', () => {
  const buf = new ArrayBuffer(1);
  new Uint8Array(buf)[0] = 2;
  const [val, consumed] = readBool(buf, 0);
  assert.equal(val, true);
  assert.equal(consumed, 1);
});

test('readBytes throws on truncated length', () => {
  assert.throws(() => readBytes(new ArrayBuffer(2), 0), RangeError);
});

test('readBytes throws on truncated payload', () => {
  const buf = new ArrayBuffer(6); // 4 bytes length + only 2 bytes
  new DataView(buf).setUint32(0, 10, true); // claims 10 bytes
  assert.throws(() => readBytes(buf, 0), RangeError);
});

test('readString throws on truncated payload', () => {
  const buf = new ArrayBuffer(5); // 4 bytes length + only 1 byte
  new DataView(buf).setUint32(0, 100, true); // claims 100 bytes
  assert.throws(() => readString(buf, 0), RangeError);
});

test('parseDrainBlob with empty buffer returns empty array (documented: no header)', () => {
  // Finding #43: empty ArrayBuffer means no data, no wire format header.
  // This is by design — an empty drain has no frame count prefix.
  const frames = parseDrainBlob(new ArrayBuffer(0));
  assert.strictEqual(frames.length, 0);
});
