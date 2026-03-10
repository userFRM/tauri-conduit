/**
 * Binary decode/encode helpers for tauri-conduit wire format.
 *
 * Each read function takes an ArrayBuffer and an offset, returning
 * a tuple of [value, bytesConsumed].
 *
 * Each write function takes a value and returns a Uint8Array.
 */

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

// ── Read helpers ────────────────────────────────────────────────

export function readU8(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 1 > buf.byteLength) throw new RangeError(`readU8: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 1);
  return [view.getUint8(0), 1];
}

export function readU16LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 2 > buf.byteLength) throw new RangeError(`readU16LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 2);
  return [view.getUint16(0, true), 2];
}

export function readU32LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 4 > buf.byteLength) throw new RangeError(`readU32LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 4);
  return [view.getUint32(0, true), 4];
}

export function readU64LE(
  buf: ArrayBuffer,
  offset: number,
): [bigint, number] {
  if (offset + 8 > buf.byteLength) throw new RangeError(`readU64LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 8);
  return [view.getBigUint64(0, true), 8];
}

export function readI8(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 1 > buf.byteLength) throw new RangeError(`readI8: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 1);
  return [view.getInt8(0), 1];
}

export function readI16LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 2 > buf.byteLength) throw new RangeError(`readI16LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 2);
  return [view.getInt16(0, true), 2];
}

export function readI32LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 4 > buf.byteLength) throw new RangeError(`readI32LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 4);
  return [view.getInt32(0, true), 4];
}

export function readI64LE(
  buf: ArrayBuffer,
  offset: number,
): [bigint, number] {
  if (offset + 8 > buf.byteLength) throw new RangeError(`readI64LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 8);
  return [view.getBigInt64(0, true), 8];
}

export function readF32LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 4 > buf.byteLength) throw new RangeError(`readF32LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 4);
  return [view.getFloat32(0, true), 4];
}

export function readF64LE(
  buf: ArrayBuffer,
  offset: number,
): [number, number] {
  if (offset + 8 > buf.byteLength) throw new RangeError(`readF64LE: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 8);
  return [view.getFloat64(0, true), 8];
}

export function readBool(
  buf: ArrayBuffer,
  offset: number,
): [boolean, number] {
  if (offset + 1 > buf.byteLength) throw new RangeError(`readBool: out of bounds (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 1);
  return [view.getUint8(0) !== 0, 1];
}

/**
 * Read a length-prefixed byte array: 4-byte LE length + raw bytes.
 */
export function readBytes(
  buf: ArrayBuffer,
  offset: number,
): [Uint8Array, number] {
  if (offset + 4 > buf.byteLength) throw new RangeError(`readBytes: out of bounds reading length prefix (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 4);
  const len = view.getUint32(0, true);
  if (offset + 4 + len > buf.byteLength) throw new RangeError(`readBytes: out of bounds reading payload (offset=${offset}, len=${len}, byteLength=${buf.byteLength})`);
  const bytes = new Uint8Array(buf.slice(offset + 4, offset + 4 + len));
  return [bytes, 4 + len];
}

/**
 * Read a length-prefixed UTF-8 string: 4-byte LE length + UTF-8 bytes.
 */
export function readString(
  buf: ArrayBuffer,
  offset: number,
): [string, number] {
  if (offset + 4 > buf.byteLength) throw new RangeError(`readString: out of bounds reading length prefix (offset=${offset}, byteLength=${buf.byteLength})`);
  const view = new DataView(buf, offset, 4);
  const len = view.getUint32(0, true);
  if (offset + 4 + len > buf.byteLength) throw new RangeError(`readString: out of bounds reading payload (offset=${offset}, len=${len}, byteLength=${buf.byteLength})`);
  const bytes = new Uint8Array(buf, offset + 4, len);
  return [textDecoder.decode(bytes), 4 + len];
}

// ── Write helpers ───────────────────────────────────────────────

export function writeU8(value: number): Uint8Array {
  const buf = new Uint8Array(1);
  buf[0] = value;
  return buf;
}

export function writeU16LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(2);
  new DataView(buf).setUint16(0, value, true);
  return new Uint8Array(buf);
}

export function writeU32LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setUint32(0, value, true);
  return new Uint8Array(buf);
}

export function writeU64LE(value: bigint): Uint8Array {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setBigUint64(0, value, true);
  return new Uint8Array(buf);
}

export function writeI8(value: number): Uint8Array {
  const buf = new ArrayBuffer(1);
  new DataView(buf).setInt8(0, value);
  return new Uint8Array(buf);
}

export function writeI16LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(2);
  new DataView(buf).setInt16(0, value, true);
  return new Uint8Array(buf);
}

export function writeI32LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setInt32(0, value, true);
  return new Uint8Array(buf);
}

export function writeI64LE(value: bigint): Uint8Array {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setBigInt64(0, value, true);
  return new Uint8Array(buf);
}

export function writeF32LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(4);
  new DataView(buf).setFloat32(0, value, true);
  return new Uint8Array(buf);
}

export function writeF64LE(value: number): Uint8Array {
  const buf = new ArrayBuffer(8);
  new DataView(buf).setFloat64(0, value, true);
  return new Uint8Array(buf);
}

export function writeBool(value: boolean): Uint8Array {
  return new Uint8Array([value ? 1 : 0]);
}

/**
 * Write a length-prefixed byte array: 4-byte LE length + raw bytes.
 */
export function writeBytes(data: Uint8Array): Uint8Array {
  const header = writeU32LE(data.byteLength);
  const result = new Uint8Array(4 + data.byteLength);
  result.set(header, 0);
  result.set(data, 4);
  return result;
}

/**
 * Write a length-prefixed UTF-8 string: 4-byte LE length + UTF-8 bytes.
 */
export function writeString(value: string): Uint8Array {
  const encoded = textEncoder.encode(value);
  const header = writeU32LE(encoded.byteLength);
  const result = new Uint8Array(4 + encoded.byteLength);
  result.set(header, 0);
  result.set(encoded, 4);
  return result;
}

/**
 * Parse the drain wire format into individual frames.
 *
 * The wire format is: `[u32 LE frame_count]` followed by
 * `[u32 LE len][bytes]` for each frame. An empty ArrayBuffer
 * (byteLength === 0) means no frames.
 *
 * Corresponds to the Rust `RingBuffer::drain_all()` / `Queue::drain_all()` output.
 */
export function parseDrainBlob(buf: ArrayBuffer): Uint8Array[] {
  if (buf.byteLength === 0) return [];
  const view = new DataView(buf);
  if (buf.byteLength < 4) return [];
  const count = view.getUint32(0, true);
  const frames: Uint8Array[] = [];
  let offset = 4;
  for (let i = 0; i < count; i++) {
    if (offset + 4 > buf.byteLength) throw new RangeError(`parseDrainBlob: truncated frame header at index ${i}`);
    const len = view.getUint32(offset, true);
    offset += 4;
    if (offset + len > buf.byteLength) throw new RangeError(`parseDrainBlob: truncated frame payload at index ${i}`);
    frames.push(new Uint8Array(buf.slice(offset, offset + len)));
    offset += len;
  }
  return frames;
}
