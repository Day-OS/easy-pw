use std::{
    fmt::Display,
    rc::Rc,
    sync::{mpsc, Arc, RwLock},
};

use futures::executor::block_on;
use pipewire::{core::Core, registry::Registry};

use super::objects::PipeWireObjects;

/// Events that is received by the main thread.
#[derive(Debug, PartialEq, Clone)]
pub enum ConnectorEvent {
    None,
    LinkUpdate(u32, u32),
    LinkFailed(u32, u32),
    UnlinkUpdate(u32, u32),
    UnLinkFailed(u32, u32),
}

/// Events that is received by the PipeWire Backend thread.
#[derive(Debug, PartialEq)]
pub enum PipeWireEvent {
    LinkCommand(u32, u32),
    UnlinkCommand(u32, u32),
}

impl Display for PipeWireEvent {
    fn fmt(
        &self,
        f: &mut std::fmt::Formatter<'_>,
    ) -> std::fmt::Result {
        match self {
            PipeWireEvent::LinkCommand(source_id, target_id) => {
                write!(f, "LinkCommand({source_id}, {target_id})")
            }
            PipeWireEvent::UnlinkCommand(source_id, target_id) => {
                write!(f, "UnlinkCommand({source_id}, {target_id})")
            }
        }
    }
}

impl PipeWireEvent {
    #[allow(unreachable_patterns)]
    /// Handle the event and return a ConnectorEvent response if needed.
    pub fn handle(
        &self,
        _event_locker: Arc<RwLock<()>>,
        objects: Arc<RwLock<PipeWireObjects>>,
        core: Rc<RwLock<Core>>,
        sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
        registry: Rc<RwLock<Registry>>,
    ) -> Result<(), ConnectorEvent> {
        let event_locker = _event_locker.write().unwrap();
        log::debug!("(Pipewire) Handling Event: {self:#?}");
        match self {
            PipeWireEvent::LinkCommand(source_id, target_id) => {
                let result = &PipeWireEvent::_link_command(
                    objects, core, *source_id, *target_id,
                );
                if let Err(e) = result {
                    log::error!("Failed to link nodes: {e}");
                    return Err(ConnectorEvent::LinkFailed(
                        *source_id, *target_id,
                    ));
                }
            }
            PipeWireEvent::UnlinkCommand(source_id, target_id) => {
                log::info!(
                    "Unlinking nodes {source_id} and {target_id}"
                );
                let result = &PipeWireEvent::_unlink_command(
                    objects,
                    registry,
                    *source_id,
                    *target_id,
                    sender.clone(),
                );
                if let Err(e) = result {
                    log::error!("Failed to link nodes: {e}");
                    return Err(ConnectorEvent::UnLinkFailed(
                        *source_id, *target_id,
                    ));
                }
            }
            _ => {
                log::warn!("Unhandled event: {self:?}");
            }
        }
        drop(event_locker);
        Ok(())
    }

    fn _link_command(
        objects: Arc<RwLock<PipeWireObjects>>,
        core: Rc<RwLock<Core>>,
        source_id: u32,
        target_id: u32,
    ) -> Result<(), String> {
        let objects = objects.write();

        if let Err(e) = objects {
            return Err(format!("Failed to lock objects: {e}"));
        }
        let mut objects = objects.unwrap();

        if source_id == target_id {
            return Err(format!(
                "Source and target IDs are the same: {source_id}"
            ));
        }

        let (input_node, target_node) =
            objects.find_two_nodes_by_id_mut(source_id, target_id);

        if input_node.is_none() || target_node.is_none() {
            return Err(format!(
                "One or both nodes not found for IDs: {source_id} and {target_id}"
            ));
        }

        let input_node = input_node.unwrap();
        let target_node = target_node.unwrap();
        if let Err(e) = input_node.link_device(core, target_node) {
            return Err(format!("Failed to link devices: {e}"));
        }
        Ok(())
    }
    fn _unlink_command(
        objects: Arc<RwLock<PipeWireObjects>>,
        registry: Rc<RwLock<Registry>>,
        source_id: u32,
        target_id: u32,
        sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
    ) -> Result<(), String> {
        let objects = objects.write();
        if let Err(e) = objects {
            return Err(format!("Failed to lock objects: {e}"));
        }
        let mut objects = objects.unwrap();

        let mut links_id = vec![];

        for link in objects.links.iter() {
            if link.output_node == source_id
                && link.input_node == target_id
            {
                links_id.push(link.id);
            }
        }

        // HOTFIX FOR WHEN SOMETHING IS NOT EVEN LINKED
        // TODO: MAKE AN ACTUAL GOOD FIX FOR THAT
        // https://github.com/Day-OS/easy-pw/issues/1
        if links_id.is_empty() {
            log::debug!("hm!");
            let _result = sender
                .read()
                .map_err(|_| "Remove Link Sender is Poisoned")?
                .send(ConnectorEvent::UnlinkUpdate(
                    source_id, target_id,
                ));
            return Ok(());
        }

        for id in links_id {
            log::debug!("Found link with ID: {id} while searching for source ID: {source_id} and target ID: {target_id}");
            block_on(objects.remove_link(
                id,
                Some(registry.clone()),
                sender.clone(),
            ))?;
        }
        Ok(())
    }
}
