use soroban_sdk::contracterror;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[contracterror]
#[repr(u32)]
pub enum NttManagerError {
    // Message parsing errors
    MessageTooShort = 1,
    InvalidPrefix = 2,

    // Admin/auth errors
    Unauthorized = 10,
    InvalidPendingAdmin = 11,
}
