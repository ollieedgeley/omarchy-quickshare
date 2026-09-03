use aes::Aes256;
use cbc::cipher::{
    BlockDecryptMut as _, BlockEncryptMut as _, KeyIvInit as _,
    block_padding::Pkcs7,
};
use hkdf::Hkdf;
use hmac::{Hmac, Mac as _};
use p256::{PublicKey, SecretKey, elliptic_curve::sec1::ToEncodedPoint as _};
use prost::Message as _;
use quickshare_wire::secure_message as securemessage;
use sha2::{Digest as _, Sha256};

use crate::secure_channel::Keys;
use crate::{CryptoError, HandshakeError, IV_LENGTH, KEY_LENGTH};

const D2D_SALT: &[u8] = b"D2D";
const SECURE_MESSAGE_SALT: &[u8] = b"SecureMessage";

#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn derive<const N: usize>(
    key: &[u8],
    salt: &[u8],
    info: &[u8],
) -> Result<[u8; N], hkdf::InvalidLength> {
    let mut output = [0; N];
    Hkdf::<Sha256>::new(Some(salt), key).expand(info, &mut output)?;
    Ok(output)
}
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn d2d_key(
    master: &[u8; KEY_LENGTH],
    purpose: &[u8],
) -> Result<[u8; KEY_LENGTH], hkdf::InvalidLength> {
    derive(master, &Sha256::digest(D2D_SALT), purpose)
}
#[must_use]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn keys(master: [u8; KEY_LENGTH]) -> Keys {
    Keys {
        encryption: derive(
            &master,
            &Sha256::digest(SECURE_MESSAGE_SALT),
            b"ENC:2",
        )
        .expect("fixed HKDF length"),
        signing: derive(
            &master,
            &Sha256::digest(SECURE_MESSAGE_SALT),
            b"SIG:1",
        )
        .expect("fixed HKDF length"),
        master,
    }
}
#[must_use]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn hmac(key: &[u8; KEY_LENGTH], data: &[u8]) -> [u8; KEY_LENGTH] {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("valid HMAC key");
    mac.update(data);
    mac.finalize().into_bytes().into()
}
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn encrypt(
    key: &[u8; KEY_LENGTH],
    iv: &[u8; IV_LENGTH],
    plain: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut buffer = plain.to_vec();
    let position = buffer.len();
    buffer.resize(position.saturating_add(IV_LENGTH), 0);
    cbc::Encryptor::<Aes256>::new(key.into(), iv.into())
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, position)
        .map(<[u8]>::to_vec)
        .map_err(|_| CryptoError::Decryption)
}
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn decrypt(
    key: &[u8; KEY_LENGTH],
    iv: &[u8; IV_LENGTH],
    ciphertext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut buffer = ciphertext.to_vec();
    cbc::Decryptor::<Aes256>::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map(<[u8]>::to_vec)
        .map_err(|_| CryptoError::Decryption)
}
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn public_key(
    secret: &SecretKey,
) -> Result<securemessage::GenericPublicKey, HandshakeError> {
    let encoded = secret.public_key().to_encoded_point(false);
    let bytes = encoded.as_bytes();
    Ok(securemessage::GenericPublicKey {
        r#type: securemessage::PublicKeyType::EcP256 as i32,
        ec_p256_public_key: Some(securemessage::EcP256PublicKey {
            x: positive(bytes.get(1..33).ok_or(HandshakeError::PublicKey)?)?,
            y: positive(bytes.get(33..65).ok_or(HandshakeError::PublicKey)?)?,
        }),
        rsa2048_public_key: None,
        dh2048_public_key: None,
    })
}
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn parse_public_key(
    bytes: &[u8],
) -> Result<PublicKey, HandshakeError> {
    let key = securemessage::GenericPublicKey::decode(bytes)
        .map_err(|_| HandshakeError::PublicKey)?;
    if key.r#type != securemessage::PublicKeyType::EcP256 as i32 {
        return Err(HandshakeError::PublicKey);
    }
    let point = key.ec_p256_public_key.ok_or(HandshakeError::PublicKey)?;
    let mut encoded = [0; 65];
    encoded[0] = 4;
    encoded[1..33].copy_from_slice(&coordinate(&point.x)?);
    encoded[33..].copy_from_slice(&coordinate(&point.y)?);
    PublicKey::from_sec1_bytes(&encoded).map_err(|_| HandshakeError::PublicKey)
}
#[must_use]
#[expect(
    clippy::redundant_pub_crate,
    reason = "Crypto siblings share it; rustc rejects public reachability"
)]
pub(super) fn java_hash(bytes: &[u8; KEY_LENGTH]) -> i32 {
    bytes.iter().fold(1_i32, |value, byte| {
        value
            .wrapping_mul(31)
            .wrapping_add(i32::from(byte.cast_signed()))
    })
}
fn positive(value: &[u8]) -> Result<Vec<u8>, HandshakeError> {
    if value.len() != KEY_LENGTH {
        return Err(HandshakeError::PublicKey);
    }
    let mut result = Vec::from(value);
    if value.first().is_some_and(|byte| byte & 0x80 != 0) {
        result.insert(0, 0);
    }
    Ok(result)
}
fn coordinate(value: &[u8]) -> Result<[u8; KEY_LENGTH], HandshakeError> {
    value
        .strip_prefix(&[0])
        .unwrap_or(value)
        .try_into()
        .map_err(|_| HandshakeError::PublicKey)
}
