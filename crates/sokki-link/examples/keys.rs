//! Keys for tests. Without an argument: a new key pair, the secret (hex)
//! and the public id. With a secret (hex): its public id, e.g. to make the
//! pairing link of a test app from its `phone_link.json`
//! (`sokki://pair#sokki1:<id>:<code>`).
fn main() {
    let k = match std::env::args().nth(1) {
        Some(hex) => sokki_link::secret_from_hex(hex.trim()).expect("a secret in hex"),
        None => sokki_link::new_secret(),
    };
    println!("{} {}", sokki_link::secret_to_hex(&k), k.public());
}
