use criterion::{black_box, criterion_group, criterion_main, Criterion};

use agentzero_core::privacy::envelope::{compute_routing_id, generate_keypair, SealedEnvelope};
use agentzero_core::privacy::noise::{NoiseHandshaker, NoiseKeypair};

const MAX_NOISE_MSG_LEN: usize = 65535;

/// Complete an XX handshake between two keypairs and return transport sessions.
fn xx_handshake_to_transport(
    client_kp: &NoiseKeypair,
    server_kp: &NoiseKeypair,
) -> (
    agentzero_core::privacy::noise::NoiseSession,
    agentzero_core::privacy::noise::NoiseSession,
) {
    let mut client = NoiseHandshaker::new_initiator("XX", client_kp).unwrap();
    let mut server = NoiseHandshaker::new_responder("XX", server_kp).unwrap();
    let mut buf = [0u8; MAX_NOISE_MSG_LEN];
    let mut payload = [0u8; MAX_NOISE_MSG_LEN];

    let len = client.write_message(b"", &mut buf).unwrap();
    server.read_message(&buf[..len], &mut payload).unwrap();
    let len = server.write_message(b"", &mut buf).unwrap();
    client.read_message(&buf[..len], &mut payload).unwrap();
    let len = client.write_message(b"", &mut buf).unwrap();
    server.read_message(&buf[..len], &mut payload).unwrap();

    (
        client.into_transport().unwrap(),
        server.into_transport().unwrap(),
    )
}

fn bench_noise_keypair_generate(c: &mut Criterion) {
    c.bench_function("noise_keypair_generate", |b| {
        b.iter(|| {
            black_box(NoiseKeypair::generate().unwrap());
        });
    });
}

fn bench_noise_xx_handshake(c: &mut Criterion) {
    c.bench_function("noise_xx_handshake", |b| {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();
        b.iter(|| {
            let mut client = NoiseHandshaker::new_initiator("XX", &client_kp).unwrap();
            let mut server = NoiseHandshaker::new_responder("XX", &server_kp).unwrap();
            let mut buf = [0u8; MAX_NOISE_MSG_LEN];
            let mut payload = [0u8; MAX_NOISE_MSG_LEN];

            // 3 messages: → e, ← e,ee,s,es, → s,se
            let len = client.write_message(b"", &mut buf).unwrap();
            server.read_message(&buf[..len], &mut payload).unwrap();
            let len = server.write_message(b"", &mut buf).unwrap();
            client.read_message(&buf[..len], &mut payload).unwrap();
            let len = client.write_message(b"", &mut buf).unwrap();
            server.read_message(&buf[..len], &mut payload).unwrap();

            let cs = client.into_transport().unwrap();
            let ss = server.into_transport().unwrap();
            black_box((cs, ss));
        });
    });
}

fn bench_noise_ik_handshake(c: &mut Criterion) {
    c.bench_function("noise_ik_handshake", |b| {
        let client_kp = NoiseKeypair::generate().unwrap();
        let server_kp = NoiseKeypair::generate().unwrap();
        b.iter(|| {
            let mut client =
                NoiseHandshaker::new_ik_initiator(&client_kp, &server_kp.public).unwrap();
            let mut server = NoiseHandshaker::new_responder("IK", &server_kp).unwrap();
            let mut buf = [0u8; MAX_NOISE_MSG_LEN];
            let mut payload = [0u8; MAX_NOISE_MSG_LEN];

            // 2 messages: → e,es,s,ss, ← e,ee,se
            let len = client.write_message(b"", &mut buf).unwrap();
            server.read_message(&buf[..len], &mut payload).unwrap();
            let len = server.write_message(b"", &mut buf).unwrap();
            client.read_message(&buf[..len], &mut payload).unwrap();

            let cs = client.into_transport().unwrap();
            let ss = server.into_transport().unwrap();
            black_box((cs, ss));
        });
    });
}

fn bench_noise_encrypt(c: &mut Criterion) {
    let client_kp = NoiseKeypair::generate().unwrap();
    let server_kp = NoiseKeypair::generate().unwrap();

    let data_64b = vec![0xABu8; 64];
    let data_1kb = vec![0xABu8; 1024];
    let data_64kb = vec![0xABu8; 64 * 1024];

    // Each iteration needs a fresh session because Noise transport is stateful
    // (nonce increments). We re-create sessions per iteration.

    c.bench_function("noise_encrypt_64b", |b| {
        b.iter_batched(
            || xx_handshake_to_transport(&client_kp, &server_kp),
            |(mut client_session, _server_session)| {
                black_box(client_session.encrypt(&data_64b).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("noise_encrypt_1kb", |b| {
        b.iter_batched(
            || xx_handshake_to_transport(&client_kp, &server_kp),
            |(mut client_session, _server_session)| {
                black_box(client_session.encrypt(&data_1kb).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("noise_encrypt_64kb", |b| {
        b.iter_batched(
            || xx_handshake_to_transport(&client_kp, &server_kp),
            |(mut client_session, _server_session)| {
                black_box(client_session.encrypt(&data_64kb).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_noise_decrypt(c: &mut Criterion) {
    let client_kp = NoiseKeypair::generate().unwrap();
    let server_kp = NoiseKeypair::generate().unwrap();

    let data_64b = vec![0xABu8; 64];
    let data_1kb = vec![0xABu8; 1024];
    let data_64kb = vec![0xABu8; 64 * 1024];

    c.bench_function("noise_decrypt_64b", |b| {
        b.iter_batched(
            || {
                let (mut cs, ss) = xx_handshake_to_transport(&client_kp, &server_kp);
                let ct = cs.encrypt(&data_64b).unwrap();
                (ss, ct)
            },
            |(mut server_session, ciphertext)| {
                black_box(server_session.decrypt(&ciphertext).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("noise_decrypt_1kb", |b| {
        b.iter_batched(
            || {
                let (mut cs, ss) = xx_handshake_to_transport(&client_kp, &server_kp);
                let ct = cs.encrypt(&data_1kb).unwrap();
                (ss, ct)
            },
            |(mut server_session, ciphertext)| {
                black_box(server_session.decrypt(&ciphertext).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });

    c.bench_function("noise_decrypt_64kb", |b| {
        b.iter_batched(
            || {
                let (mut cs, ss) = xx_handshake_to_transport(&client_kp, &server_kp);
                let ct = cs.encrypt(&data_64kb).unwrap();
                (ss, ct)
            },
            |(mut server_session, ciphertext)| {
                black_box(server_session.decrypt(&ciphertext).unwrap());
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

fn bench_sealed_envelope_seal_open(c: &mut Criterion) {
    c.bench_function("sealed_envelope_seal_open", |b| {
        let (pubkey, secret) = generate_keypair();
        let plaintext = b"benchmark sealed envelope payload";
        b.iter(|| {
            let envelope = SealedEnvelope::seal(&pubkey, black_box(plaintext), 3600);
            let decrypted = envelope.open(&secret).unwrap();
            black_box(decrypted);
        });
    });
}

fn bench_compute_routing_id(c: &mut Criterion) {
    c.bench_function("compute_routing_id", |b| {
        let pubkey = [42u8; 32];
        b.iter(|| {
            black_box(compute_routing_id(black_box(&pubkey)));
        });
    });
}

criterion_group!(
    privacy,
    bench_noise_keypair_generate,
    bench_noise_xx_handshake,
    bench_noise_ik_handshake,
    bench_noise_encrypt,
    bench_noise_decrypt,
    bench_sealed_envelope_seal_open,
    bench_compute_routing_id,
);
criterion_main!(privacy);
