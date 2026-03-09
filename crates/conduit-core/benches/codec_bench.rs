use criterion::{Criterion, black_box, criterion_group, criterion_main};

use conduit_core::{
    FRAME_HEADER_SIZE, FrameHeader, MsgType, PROTOCOL_VERSION, Decode, Encode,
    frame_unpack, frame_pack,
};

fn header_roundtrip(c: &mut Criterion) {
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Request,
        sequence: 42,
        payload_len: 128,
    };
    let mut buf = Vec::with_capacity(FRAME_HEADER_SIZE);

    c.bench_function("FrameHeader write_to + read_from", |b| {
        b.iter(|| {
            buf.clear();
            header.write_to(&mut buf);
            let parsed = FrameHeader::read_from(black_box(&buf)).unwrap();
            black_box(parsed);
        });
    });
}

fn frame_pack_unwrap(c: &mut Criterion) {
    let header = FrameHeader {
        version: PROTOCOL_VERSION,
        reserved: 0,
        msg_type: MsgType::Response,
        sequence: 1,
        payload_len: 0, // will be overwritten per-iteration
    };

    for size in [0, 64, 1024, 64 * 1024] {
        let label = if size >= 1024 {
            format!("frame_pack+unwrap {}KB", size / 1024)
        } else {
            format!("frame_pack+unwrap {}B", size)
        };
        let payload = vec![0xABu8; size];
        let hdr = FrameHeader {
            payload_len: size as u32,
            ..header
        };

        c.bench_function(&label, |b| {
            b.iter(|| {
                let frame = frame_pack(black_box(&hdr), black_box(&payload));
                let (parsed_hdr, parsed_payload) = frame_unpack(black_box(&frame)).unwrap();
                black_box((parsed_hdr, parsed_payload));
            });
        });
    }
}

fn wire_primitives(c: &mut Criterion) {
    let mut buf = Vec::with_capacity(64);

    c.bench_function("Encode+Decode u64", |b| {
        b.iter(|| {
            buf.clear();
            black_box(0xDEAD_BEEF_CAFE_BABEu64).encode(&mut buf);
            let (val, _) = u64::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });

    c.bench_function("Encode+Decode f64", |b| {
        b.iter(|| {
            buf.clear();
            black_box(std::f64::consts::PI).encode(&mut buf);
            let (val, _) = f64::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });

    c.bench_function("Encode+Decode bool", |b| {
        b.iter(|| {
            buf.clear();
            black_box(true).encode(&mut buf);
            let (val, _) = bool::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });
}

fn wire_vec(c: &mut Criterion) {
    let mut buf = Vec::with_capacity(2048);

    let vec_64 = vec![0xFFu8; 64];
    c.bench_function("Encode+Decode Vec<u8> 64B", |b| {
        b.iter(|| {
            buf.clear();
            black_box(&vec_64).encode(&mut buf);
            let (val, _) = Vec::<u8>::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });

    let vec_1k = vec![0xFFu8; 1024];
    c.bench_function("Encode+Decode Vec<u8> 1KB", |b| {
        b.iter(|| {
            buf.clear();
            black_box(&vec_1k).encode(&mut buf);
            let (val, _) = Vec::<u8>::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });
}

fn wire_string(c: &mut Criterion) {
    let mut buf = Vec::with_capacity(512);

    let short = String::from("hello");
    c.bench_function("Encode+Decode String short", |b| {
        b.iter(|| {
            buf.clear();
            black_box(&short).encode(&mut buf);
            let (val, _) = String::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });

    let medium = "x".repeat(256);
    c.bench_function("Encode+Decode String 256ch", |b| {
        b.iter(|| {
            buf.clear();
            black_box(&medium).encode(&mut buf);
            let (val, _) = String::decode(black_box(&buf)).unwrap();
            black_box(val);
        });
    });
}

criterion_group!(
    benches,
    header_roundtrip,
    frame_pack_unwrap,
    wire_primitives,
    wire_vec,
    wire_string,
);
criterion_main!(benches);
