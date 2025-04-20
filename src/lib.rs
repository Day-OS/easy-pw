mod event;
mod link;
mod manager;
mod node;
pub mod objects;
mod port;
mod utils;

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use crate::manager::PipeWireManager;

    use super::*;

    #[test]
    fn creation_of_manager() {
        //Initialize PipeWire
        let _ = PipeWireManager::default();
    }
}
