//! A new key pair for tests: prints the secret (hex) and the public id.
fn main() {
    let k = sokki_link::new_secret();
    println!("{} {}", sokki_link::secret_to_hex(&k), k.public());
}
