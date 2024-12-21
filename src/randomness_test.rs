use super::*;
use std::sync::Arc;
use chrono::{TimeZone, Utc};
use twine::twine_builder::{TwineBuilder, RingSigner};
use twine::twine_core::multihash_codetable::Code;
use twine::twine_builder::pkcs8::{DecodePrivateKey, SecretDocument};
use twine::twine_core::multihash_codetable::Multihash;
use twine::twine_core::verify::{Verifiable, Verified};
use twine::twine_core::{serde_ipld_dagjson, Bytes};
use twine::twine_core::multihash_codetable::MultihashDigest;
use std::result::Result;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RandomnessPayloadRaw {
  salt: Bytes,
  pre: Multihash,
  timestamp: u64,
}

impl Verifiable for RandomnessPayloadRaw {

  fn verify(&self) -> Result<(), VerificationError> {
    if self.salt.len() != self.pre.size() as usize {
      return Err(VerificationError::Payload("Salt length does not match pre hash size".to_string()));
    }
    Ok(())
  }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RandomnessPayload(Verified<RandomnessPayloadRaw>);

impl RandomnessPayload {
  fn try_new(salt: Bytes, pre: Multihash, timestamp: u64) -> Result<Self, VerificationError> {
    Verified::try_new(RandomnessPayloadRaw { salt, pre, timestamp }).map(Self)
  }

  fn try_new_now(salt: Bytes, pre: Multihash) -> Result<Self, VerificationError> {
    Self::try_new(
      salt,
      pre,
      std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
    )
  }

  fn from_rand(rand: Vec<u8>, pre: Multihash, prev: Arc<Tixel>) -> Result<Self, VerificationError> {
    if prev.cid().hash().size() != pre.size() {
      return Err(VerificationError::Payload("Pre hash size does not match previous tixel hash size".to_string()));
    }
    // we xor the random bytes with previous cid hash digest
    let salt = Bytes(
      rand.iter()
        .zip(prev.cid().hash().digest().iter())
        .map(|(a, b)| a ^ b).collect()
    );
    Self::try_new_now(salt, pre)
  }

  fn new_start(pre: Multihash) -> Result<Self, VerificationError> {
    let num_bytes = pre.size();
    let salt = Bytes((0..num_bytes).collect());
    Self::try_new_now(salt, pre)
  }

  fn validate_randomness(&self, prev: Arc<Tixel>) -> Result<(), VerificationError> {
    if prev.cid().hash().size() != self.0.pre.size() {
      return Err(VerificationError::Payload("Pre hash size does not match previous tixel hash size".to_string()));
    }
    let prev_payload = prev.extract_payload::<RandomnessPayload>()?;
    if self.0.timestamp < prev_payload.0.timestamp {
      return Err(VerificationError::Payload("Timestamp is less than previous tixel timestamp".to_string()));
    }
    // check that the precommitment from the previous tixel matches the xor rand value
    let rand = self.0.salt.iter()
      .zip(prev.cid().hash().digest().iter())
      .map(|(a, b)| a ^ b).collect::<Vec<u8>>();

    use twine::twine_core::multihash_codetable::MultihashDigest;
    let code = Code::try_from(prev_payload.0.pre.code()).map_err(|_| VerificationError::UnsupportedHashAlgorithm)?;
    let pre = code.digest(&rand);
    if pre != self.0.pre {
      return Err(VerificationError::Payload("Pre hash does not match previous tixel pre hash".to_string()));
    }
    Ok(())
  }

  fn extract_randomness(current: Arc<Tixel>, prev: Arc<Tixel>) -> Result<Vec<u8>, VerificationError> {
    let payload = current.extract_payload::<RandomnessPayload>()?;
    if let Err(e) = payload.validate_randomness(prev) {
      return Err(e);
    }
    Ok(
      current.cid().hash().digest().to_vec()
    )
  }
}

pub async fn handle_randomness_pulse(env: Env) -> std::result::Result<Twine, Box<dyn std::error::Error>> {
  let store = store::D1Store {
    db: env.d1("DB")?,
    max_batch_size: 10, // doesn't matter here
  };

  let key_str = env.var("SECRET_KEY_STR")?;
  let private_key_json = format!(r#"{{"/":{{"bytes":"{}"}}}}"#, key_str);
  let private_key : Bytes = serde_ipld_dagjson::from_slice(private_key_json.as_bytes()).unwrap();
  let signer = RingSigner::new(
    twine::twine_core::crypto::SignatureAlgorithm::Ed25519,
    SecretDocument::from_pkcs8_der(&private_key.0)?
  ).unwrap();

  let builder = TwineBuilder::new(signer);
  // this should be idempotent
  let strand = builder.build_strand()
    .hasher(Code::Sha3_256)
    .subspec("time/1.0.0".to_string())
    .genesis(Utc.with_ymd_and_hms(2024, 12, 20, 0, 0, 0).unwrap())
    .done()
    .unwrap();

  store.put_strand(&strand).await?;
  // console_debug!("Strand CID: {}", strand.cid());

  let latest = match store.latest(&strand.cid()).await {
    Ok(tixel) => Some(Twine::try_new_from_shared(strand.clone().into(), tixel)?),
    Err(e) => match e {
      ApiError::NotFound => None,
      _ => return Err(Box::new(e)),
    },
  };

  // console_debug!("Latest: {}", latest);

  let next = match latest {
    Some(prev) => {
      builder.build_next(&prev)
        .payload(ipld!({
          "timestamp": Utc::now().timestamp()
        }))
        .done()?
    },
    None => {
      builder.build_first(strand)
        .payload(ipld!({
          "timestamp": Utc::now().timestamp()
        }))
        .done()?
    },
  };

  store.put_tixel(&next.tixel()).await?;
  Ok(next)
}
