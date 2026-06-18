use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Identity {
    pub aud: Vec<String>,
    pub iat: usize,
    pub exp: usize,
    pub sub: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Authority {
    pub iat: usize,
    pub exp: usize,
    pub role: u128,
    pub aud: Vec<String>,
    pub sub: Uuid,
    pub rcpt: Uuid,
}

impl Identity {
    pub fn new(sub: Uuid, aud: Vec<String>) -> Self {
        let iat = Utc::now().timestamp_millis() as usize;
        Self {
            aud,
            iat,
            exp: iat + (1000 * 500),
            sub,
        }
    }
}

impl Authority {
    pub fn new(sub: Uuid, role: u128, rcpt: Uuid, aud: Vec<String>) -> Self {
        let iat = Utc::now().timestamp_millis() as usize;
        Self {
            aud,
            iat,
            exp: iat + (1000 * 500),
            sub,
            role,
            rcpt,
        }
    }

    pub fn check(&self, perm_id: u16) -> bool {
        let perm_value = 2 << perm_id;
        self.role & perm_value == perm_value
    }
}
