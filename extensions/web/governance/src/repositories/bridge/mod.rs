//! Persistence for the bridge control plane: API keys, device certificates, and
//! exchange codes.

pub mod api_keys;
pub mod device_certs;
pub mod error;
pub mod exchange_codes;

pub use api_keys::{
    ApiKeyRow, EnrollDeviceParams, EnrolledDevice, IssuedApiKey, enroll_device, issue_api_key,
    revoke_api_key, revoke_expired_api_keys_by_name_prefix,
};
pub use device_certs::{DeviceCertRow, revoke_device_cert};
pub use error::{BridgeRepoError, Result};
pub use exchange_codes::{IssuedExchangeCode, issue_exchange_code};
