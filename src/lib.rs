mod event;
mod link;
pub mod manager;
mod node;
pub mod objects;
pub mod port;
mod utils;

#[cfg(test)]
mod tests {
    use crate::manager::PipeWireManager;

    #[test]
    fn creation_of_manager() {
        //Initialize PipeWire
        let _ = PipeWireManager::default();
    }
}
