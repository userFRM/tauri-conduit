/**
 * Frame header codec for tauri-conduit.
 *
 * 11-byte binary frame header:
 *   version       (u8)     byte 0
 *   reserved      (u8)     byte 1     0 (reserved for future use)
 *   msg_type      (u8)     byte 2
 *   sequence      (u32 LE) bytes 3-6
 *   payload_len   (u32 LE) bytes 7-10
 */

export const FRAME_HEADER_SIZE = 11;
export const PROTOCOL_VERSION = 1;

export enum MsgType {
  Request = 0x00,
  Response = 0x01,
  Push = 0x02,
  Error = 0x04,
}

export interface FrameHeader {
  version: number;
  reserved: number;
  msgType: number;
  sequence: number;
  payloadLen: number;
}

/**
 * Write a frame header followed by a payload into a single ArrayBuffer.
 */
export function packFrame(
  header: FrameHeader,
  payload: Uint8Array,
): ArrayBuffer {
  const buf = new ArrayBuffer(FRAME_HEADER_SIZE + payload.byteLength);
  const view = new DataView(buf);

  view.setUint8(0, header.version);
  view.setUint8(1, header.reserved);
  view.setUint8(2, header.msgType);
  view.setUint32(3, header.sequence, true); // LE
  view.setUint32(7, header.payloadLen, true); // LE

  new Uint8Array(buf, FRAME_HEADER_SIZE).set(payload);

  return buf;
}

/**
 * Read a frame header and extract the payload from a raw ArrayBuffer.
 * Returns null if the buffer is too small for a complete frame.
 */
export function unpackFrame(
  data: ArrayBuffer,
): { header: FrameHeader; payload: ArrayBuffer } | null {
  if (data.byteLength < FRAME_HEADER_SIZE) return null;

  const view = new DataView(data);

  const header: FrameHeader = {
    version: view.getUint8(0),
    reserved: view.getUint8(1),
    msgType: view.getUint8(2),
    sequence: view.getUint32(3, true),
    payloadLen: view.getUint32(7, true),
  };

  // Validate that the buffer contains the full payload.
  const totalSize = FRAME_HEADER_SIZE + header.payloadLen;
  if (data.byteLength < totalSize) return null;

  const payload = data.slice(FRAME_HEADER_SIZE, totalSize);

  return { header, payload };
}
