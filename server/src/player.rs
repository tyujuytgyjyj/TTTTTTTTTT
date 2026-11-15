use std::net::SocketAddr;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct Player {
    pub id: u32,
    pub name: String,
    pub addr: SocketAddr,
    pub client_id: Uuid,
    pub cars: Vec<u32>,
    pub is_op: bool,
    pub can_build: bool,
}

impl Player {
    pub fn new(id: u32, name: String, addr: SocketAddr, client_id: Uuid, is_op: bool, can_build: bool) -> Self {
        Self {
            id,
            name,
            addr,
            client_id,
            cars: Vec::new(),
            is_op,
            can_build: is_op || can_build,
        }
    }
}
