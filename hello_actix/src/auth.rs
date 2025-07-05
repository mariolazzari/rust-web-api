use actix_web::{error, web};
use base64::engine::general_purpose::STANDARD_NO_PAD as BASE64;
use base64::Engine as _;
use ring::rand::SecureRandom;
use ring::{aead, rand};
use std::collections::HashMap;
use std::error::Error;
use std::fs::read_to_string;
use std::iter::repeat_with;
use std::sync::{Arc, LazyLock, RwLock};

use crate::db;

const MASTER_KEY_FILE: &str = "master.key";
const SALT_LENGTH: usize = 16;
const MASTER_KEY_LENGTH: usize = 32;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[allow(clippy::type_complexity)]
static API_KEYS: LazyLock<Arc<RwLock<HashMap<String, Vec<u8>>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

fn get_or_create_master_key() -> Result<aead::LessSafeKey> {
    let key = if let Ok(existing_key) = read_to_string(MASTER_KEY_FILE) {
        BASE64.decode(existing_key.trim())?
    } else {
        let rng = rand::SystemRandom::new();
        let mut key = [0; MASTER_KEY_LENGTH];
        rng.fill(&mut key)
            .map_err(|_| "Failed to generate random key")?;
        let encoded_key = BASE64.encode(key);
        std::fs::write(MASTER_KEY_FILE, encoded_key)?;
        key.to_vec()
    };

    if key.len() != MASTER_KEY_LENGTH {
        return Err("Invalid master key length".into());
    }

    Ok(aead::LessSafeKey::new(
        aead::UnboundKey::new(&aead::AES_256_GCM, &key).map_err(|_| "Invalid key length")?,
    ))
}

fn generate_salt() -> Result<[u8; SALT_LENGTH]> {
    let rng = rand::SystemRandom::new();
    let mut salt = [0u8; SALT_LENGTH];
    rng.fill(&mut salt).map_err(|_| "Failed to generate salt")?;
    Ok(salt)
}

fn encrypt(plaintext: &str, salt: &[u8]) -> Result<String> {
    let key = get_or_create_master_key()?;
    let nonce = aead::Nonce::assume_unique_for_key([0; 12]);
    let mut in_out = plaintext.as_bytes().to_vec();
    key.seal_in_place_append_tag(nonce, aead::Aad::from(salt), &mut in_out)
        .map_err(|_| "Encryption failed")?;
    Ok(BASE64.encode(in_out))
}

fn decrypt(ciphertext: &str, salt: &[u8]) -> Result<String> {
    let key = get_or_create_master_key()?;
    let nonce = aead::Nonce::assume_unique_for_key([0; 12]);
    let mut in_out = BASE64.decode(ciphertext)?;
    let plaintext = key
        .open_in_place(nonce, aead::Aad::from(salt), &mut in_out)
        .map_err(|_| "Decryption failed")?;
    Ok(String::from_utf8(plaintext.to_vec())?)
}

pub fn create_api_key() -> String {
    repeat_with(fastrand::alphanumeric).take(40).collect()
}

pub fn load_api_keys(database: web::Data<db::Pool>) -> Result<()> {
    let conn = database
        .get()
        .map_err(actix_web::error::ErrorInternalServerError)?;

    let mut stmt = conn.prepare(
        "
        SELECT  api_key, salt
        FROM    api_keys
        WHERE   revoked_at IS NULL
    ;",
    )?;

    let mut rows = stmt.query(()).map_err(error::ErrorInternalServerError)?;

    let mut api_keys = API_KEYS.write().unwrap();

    while let Some(row) = rows.next().map_err(error::ErrorInternalServerError)? {
        let api_key: String = row.get(0).map_err(error::ErrorInternalServerError)?;
        let salt: String = row.get(1).map_err(error::ErrorInternalServerError)?;

        let salt = BASE64.decode(salt)?;

        let api_key = decrypt(&api_key, &salt)?;
        api_keys.insert(api_key, salt);
    }

    Ok(())
}

pub async fn store_api_key(database: web::Data<db::Pool>, api_key: impl AsRef<str>) -> Result<()> {
    let salt = generate_salt()?;
    let api_key = encrypt(api_key.as_ref(), &salt)?;
    let salt = BASE64.encode(salt);
    let query = db::Query::StoreApiKey { salt, api_key };

    query.execute(database.clone()).await?;

    load_api_keys(database.clone())
}

pub async fn revoke_api_key(database: web::Data<db::Pool>, token: String) -> Result<()> {
    let query = db::Query::RevokeApiKey(token);
    query.execute(database.clone()).await?;

    load_api_keys(database.clone())
}

pub fn is_key_allowed_access(api_key: &str) -> Result<bool> {
    let api_keys = API_KEYS.read()?;

    Ok(api_keys.contains_key(api_key))
}
