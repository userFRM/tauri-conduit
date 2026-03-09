import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  FRAME_HEADER_SIZE,
  PROTOCOL_VERSION,
  MsgType,
  packFrame,
  unpackFrame,
} from '../codec/frame.js';

describe('frame codec', () => {
  it('FRAME_HEADER_SIZE is 11', () => {
    assert.equal(FRAME_HEADER_SIZE, 11);
  });

  it('PROTOCOL_VERSION is 1', () => {
    assert.equal(PROTOCOL_VERSION, 1);
  });

  it('MsgType enum values', () => {
    assert.equal(MsgType.Request, 0x00);
    assert.equal(MsgType.Response, 0x01);
    assert.equal(MsgType.Push, 0x02);
    assert.equal(MsgType.Error, 0x04);
  });

  it('packFrame + unpackFrame roundtrip', () => {
    const header = {
      version: PROTOCOL_VERSION,
      reserved: 0,
      msgType: MsgType.Request,
      sequence: 42,
      payloadLen: 128,
    };
    const payload = new Uint8Array(128);
    const buf = packFrame(header, payload);
    const parsed = unpackFrame(buf);
    assert.ok(parsed);
    assert.equal(parsed.header.version, PROTOCOL_VERSION);
    assert.equal(parsed.header.msgType, MsgType.Request);
    assert.equal(parsed.header.sequence, 42);
    assert.equal(parsed.header.payloadLen, 128);
  });

  it('unpackFrame returns null for short buffer', () => {
    const buf = new ArrayBuffer(5);
    const parsed = unpackFrame(buf);
    assert.equal(parsed, null);
  });
});
