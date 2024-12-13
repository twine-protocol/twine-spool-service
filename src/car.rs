use futures::AsyncRead;
use fvm_ipld_car::CarReader;
use crate::ApiError;

use super::*;

pub async fn car_to_single_twine<R: AsyncRead + Send + Unpin, T: TwineBlock>(car: R) -> std::result::Result<T, ApiError> {
  let mut reader = CarReader::new_unchecked(car).await.map_err(|e| ApiError::BadRequestData(e.to_string()))?;
  let block = reader.next_block().await
    .map_err(|e| ApiError::BadRequestData(e.to_string()))?
    .ok_or(ApiError::BadRequestData("No blocks in car".to_string()))?;
  let cid = Cid::try_from(block.cid.to_bytes()).unwrap();
  let twine = T::from_block(cid, block.data)?;
  Ok(twine)
}

pub async fn car_to_twines<R: AsyncRead + Send + Unpin, T: TwineBlock>(car: R) -> std::result::Result<Vec<T>, ApiError> {
  let mut reader = CarReader::new_unchecked(car).await.map_err(|e| ApiError::BadRequestData(e.to_string()))?;
  let mut twines = vec![];
  while let Some(block) = reader.next_block().await
    .map_err(|e| ApiError::BadRequestData(e.to_string()))?
  {
    let cid = Cid::try_from(block.cid.to_bytes()).unwrap();
    let twine = T::from_block(cid, block.data)?;
    twines.push(twine);
  }
  Ok(twines)
}