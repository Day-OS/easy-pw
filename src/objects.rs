use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{mpsc, Arc, RwLock};

use pipewire::registry::Registry;

use crate::event::ConnectorEvent;

use super::link::Link;
use super::node::Node;
use super::port::Port;
use futures::executor;
#[derive(Default)]
pub struct PipeWireObjects {
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub(super) _ports_to_be_added: Vec<Port>,
}

impl PipeWireObjects {
    pub fn update_nodes(&mut self) {
        let mut nodes: HashMap<u32, (&mut Node, bool)> =
            HashMap::new();
        // Fill nodes
        if self.nodes.is_empty() || self._ports_to_be_added.is_empty()
        {
            return;
        }
        log::debug!("Nodes Quantity: {:?}", self.nodes.len());
        log::debug!(
            "Ports that need to be added: {:?}",
            self._ports_to_be_added.len()
        );
        for node in self.nodes.iter_mut() {
            nodes.insert(node.id, (node, false));
        }

        let mut ports_not_found: Vec<Port> = vec![];
        while let Some(port) = self._ports_to_be_added.pop() {
            let port_id = port.id;
            let node_id = port.node_id;

            if let Some(node) = nodes.get_mut(&node_id) {
                if node.0.has_port(&port) {
                    continue;
                }
                log::debug!(
                    "Adding port {port_id} to node {node_id}"
                );
                node.0.add_port(port);
                node.1 = true;
            } else {
                log::error!("Port {port_id} has no node");
                ports_not_found.push(port);
            }
        }

        // If the port was not found, then we reintegrate it into our ports_to_be_added list
        // That makes sure that it will not be deleted at this time
        self._ports_to_be_added.extend(ports_not_found);

        for (_, (node, updated)) in nodes.iter() {
            if !updated {
                continue;
            }
            log::debug!(
                "Node {}({}) was updated | Ports: {:#?}",
                node.name,
                node.id,
                node.get_port_names()
            );
        }

        // DEBUG ALL NODES:
        // let str_nodes: Vec<String> = self.nodes.iter().map(|node| format!("Node {}({}) | Ports: {:#?}",
        // node.name,
        // node.id,
        // node.get_port_names())).collect();
        // log::debug!("{:#?}", str_nodes);
    }

    pub fn find_node_by_id(&self, id: u32) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|node| node.id == id || node.has_port_of_id(id))
    }

    #[allow(dead_code)]
    pub fn find_node_by_id_mut(
        &mut self,
        id: u32,
    ) -> Option<&mut Node> {
        self.nodes
            .iter_mut()
            .find(|node| node.id == id || node.has_port_of_id(id))
    }

    pub fn find_two_nodes_by_id_mut(
        &mut self,
        first_id: u32,
        second_id: u32,
    ) -> (Option<&mut Node>, Option<&mut Node>) {
        let mut first: Option<&mut Node> = None;
        let mut second: Option<&mut Node> = None;

        for node in &mut self.nodes {
            if node.id == first_id {
                first = Some(node);
            } else if node.id == second_id {
                second = Some(node)
            }
        }
        (first, second)
    }

    #[allow(dead_code)]
    pub fn find_links_by_id(&self, id: u32) -> Option<&Link> {
        self.links.iter().find(|link| link.id == id)
    }

    pub fn find_linked_nodes_by_link_id_mut(
        &mut self,
        id: u32,
    ) -> Option<(u32, u32)> {
        let link = self.links.iter_mut().find(|link| link.id == id);
        link.map(|link| (link.output_node, link.input_node))
    }

    pub fn find_node_id_by_name(&self, name: &str) -> Option<u32> {
        let node = self.nodes.iter().find(|node| node.name == name);
        node.map(|node| node.id)
    }

    pub fn remove_node(&mut self, id: u32) {
        if let Some(index) =
            self.nodes.iter().position(|n| n.id == id)
        {
            self.nodes.remove(index);
        }
    }
    #[allow(dead_code)]
    pub fn print_nodes(&self) {
        self.nodes.iter().for_each(|node| {
            log::info!("=======================\nNode ID: {}, \nNode Name: {} \nNode Description {:?} \nPorts: {:?}", node.id, node.name, node.description, node.get_port_names());
        });
    }
    /// Removes a link from the list of links and optionally from the registry.
    /// If registry is None, then it will not remove the link from the registry.
    pub fn remove_link(
        &mut self,
        id: u32,
        registry: Option<Rc<RwLock<Registry>>>,
        sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
    ) -> Result<(u32, u32), String> {
        let link = self.find_linked_nodes_by_link_id_mut(id);
        if link.is_none() {
            return Err(format!("Failed to find link with id {id}"));
        }
        let link = link.unwrap();

        // Log what node is being removed from what node;
        let (input_node, output_node) = link;

        let (first_node, second_node) =
            self.find_two_nodes_by_id_mut(input_node, output_node);

        // In case this fails, it means that one of the nodes were deleted earlier.
        if first_node.is_some() && second_node.is_some() {
            let first_node = first_node.unwrap();
            let second_node = second_node.unwrap();
            log::debug!(
                "Removing the link between node {} and node {}",
                first_node.name,
                second_node.name
            );

            if registry.is_some() {
                let registry = registry.unwrap();
                executor::block_on(Link::remove_link(id, registry));
            }
        }

        let index =
            self.links.iter().position(|link| link.id == id).unwrap();
        self.links.remove(index);

        let _result = sender
            .read()
            .map_err(|_| "Remove Link Sender is Poisoned")?
            .send(ConnectorEvent::UnlinkUpdate(link.0, link.1));
        Ok(link)
    }
}
