use crate::link::Link;
use crate::node::Node;
use crate::objects::PipeWireObjects;
use crate::port::Port;
use event::{ConnectorEvent, PipeWireEvent};
use libspa::utils::dict::DictRef;
use pipewire as pw;
use pipewire::channel;
use pipewire::core::Core;
use pipewire::registry::{GlobalObject, Registry};
use std::rc::Rc;
use std::sync::mpsc::TryRecvError;
use std::sync::{mpsc, Arc, RwLock};
use std::thread;

use crate::event;

pub struct PipeWireManager {
    #[allow(dead_code)]
    pub(crate) objects: Arc<RwLock<PipeWireObjects>>,
    pub _main_thread: thread::JoinHandle<()>,
    pub _receiver: mpsc::Receiver<event::ConnectorEvent>,
    _sender: channel::Sender<event::PipeWireEvent>,
    pub _event_locker: Arc<RwLock<()>>,
}

unsafe impl Sync for PipeWireManager {}

impl Default for PipeWireManager {
    fn default() -> Self {
        let (main_sender, main_receiver) =
            mpsc::channel::<event::ConnectorEvent>();
        let (pw_sender, pw_receiver) =
            channel::channel::<event::PipeWireEvent>();
        // Store nodes in thread-safe container
        let nodes = Arc::new(RwLock::new(PipeWireObjects::default()));
        let event_locker = Arc::new(RwLock::new(()));

        Self {
            objects: nodes.clone(),
            _main_thread: Self::_start_thread(
                event_locker.clone(),
                main_sender,
                pw_receiver,
                nodes.clone(),
            ),
            _receiver: main_receiver,
            _sender: pw_sender,
            _event_locker: event_locker,
        }
    }
}

impl PipeWireManager {
    fn _start_thread(
        _event_locker: Arc<RwLock<()>>,
        _sender: mpsc::Sender<event::ConnectorEvent>,
        _receiver: channel::Receiver<event::PipeWireEvent>,
        objects: Arc<RwLock<PipeWireObjects>>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            // Initialize PipeWire
            pw::init();
            let mainloop = pw::main_loop::MainLoop::new(None)
                .expect("Failed to create main loop");
            let context = pw::context::Context::new(&mainloop)
                .expect("Failed to create context");
            let core = context
                .connect(None)
                .expect("Failed to connect to core");
            let registry =
                core.get_registry().expect("Failed to get registry");

            // Clone for use in callback
            let objects_clone = objects.clone();
            let objects_clone_remove = objects.clone();
            let objects_clone_event = objects.clone();

            let _sender_arcmtx: Arc<
                RwLock<mpsc::Sender<ConnectorEvent>>,
            > = Arc::new(RwLock::new(_sender));

            let core_lock: Rc<RwLock<Core>> =
                Rc::new(RwLock::new(core));

            let registry_lock: Rc<RwLock<Registry>> =
                Rc::new(RwLock::new(registry));

            let registry_lock_read = registry_lock.read().unwrap();

            let event_handler_sender = _sender_arcmtx.clone();
            let event_remove_handler_sender = _sender_arcmtx.clone();
            // Add registry listener
            let _listener = registry_lock_read
                .add_listener_local()
                .global(move |global| {
                    Self::_pw_event_handler(
                        global,
                        &objects_clone.clone(),
                        event_handler_sender.clone(),
                    )
                })
                .global_remove(move |object_id| {
                    Self::_pw_remove_event_handler(
                        object_id,
                        &objects_clone_remove,
                        event_remove_handler_sender.clone(),
                    )
                })
                .register();

            drop(registry_lock_read);

            let manager_events_sender = _sender_arcmtx.clone();
            let _receiver =
                _receiver.attach(mainloop.loop_(), move |event| {
                    let _sender_mtx = _sender_arcmtx.read().unwrap();
                    let objects = objects_clone_event.clone();
                    let core = core_lock.clone();
                    let event_result = event.handle(
                        _event_locker.clone(),
                        objects,
                        core,
                        manager_events_sender.clone(),
                        registry_lock.clone(),
                    );
                    if let Err(event_result) = event_result {
                        _sender_mtx.send(event_result).unwrap();
                    }
                });

            // Process events to populate nodes
            mainloop.run();
        })
    }
    fn _pw_event_handler(
        global: &GlobalObject<&DictRef>,
        objects: &Arc<RwLock<PipeWireObjects>>,
        _sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
    ) {
        // Filter by only node ones
        let mut objects_guard = objects.write().unwrap();
        let mut _sender_guard = _sender.read().unwrap();
        match global.type_ {
            pw::types::ObjectType::Node => {
                let node = Node::new(global);
                objects_guard.nodes.push(node);
            }
            pw::types::ObjectType::Port => {
                let port = Port::new(global);
                objects_guard._ports_to_be_added.push(port);
                log::debug!(
                    "(Pipewire)Received PORT event: {:?} \n{:#?}",
                    global,
                    global.props
                );
            }
            pw::types::ObjectType::Link => {
                let link = Link::new(global);
                log::debug!(
                    "(Pipewire) Received LINK event: {:?} \n{:#?}",
                    global,
                    global.props
                );
                let first_id = link.output_node;
                let second_id = link.input_node;
                objects_guard.links.push(link);
                let _result = _sender_guard.send(
                    ConnectorEvent::LinkUpdate(first_id, second_id),
                );
            }
            _ => {
                log::debug!("(Pipewire)Received non-handled event: {:?} \n{:#?}", global.type_, global.props);
                let _result =
                    _sender_guard.send(ConnectorEvent::None);
            }
        }
        objects_guard.update_nodes();
    }

    fn _pw_remove_event_handler(
        object_id: u32,
        objects: &Arc<RwLock<PipeWireObjects>>,
        _sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
    ) {
        let mut objs = objects.write().unwrap();
        PipeWireManager::remove_object(&mut objs, object_id, _sender);
    }

    fn _raise_event(&self, event: PipeWireEvent) {
        let event_info = event.to_string();
        if let Err(e) = self._sender.send(event) {
            log::error!("Failed to send event: {e:?}");
        }
        log::debug!("Event raised: {event_info:?}");
        let _thread_locker = self._event_locker.read().unwrap();
    }

    fn remove_object(
        objects: &mut PipeWireObjects,
        obj_id: u32,
        _sender: Arc<RwLock<mpsc::Sender<ConnectorEvent>>>,
    ) {
        if objects.find_linked_nodes_by_link_id_mut(obj_id).is_some()
        {
            let link = objects.remove_link(obj_id, None, _sender);
            if let Err(err) = link {
                log::error!("Failed to remove link: {err}");
                return;
            }
        }
        if let Some(node) = objects.find_node_by_id(obj_id) {
            objects.remove_node(node.id);
        }
    }

    /// Create a link between two nodes
    /// The first one should have an output port and the second one an input port
    #[allow(dead_code)]
    pub fn link_nodes(
        &self,
        first_node_id: u32,
        second_node_id: u32,
    ) {
        self._raise_event(PipeWireEvent::LinkCommand(
            first_node_id,
            second_node_id,
        ));
        self.wait_for_event(|event: &ConnectorEvent| {
            *event
                == ConnectorEvent::LinkUpdate(
                    first_node_id,
                    second_node_id,
                )
                || *event
                    == ConnectorEvent::LinkFailed(
                        first_node_id,
                        second_node_id,
                    )
        });
    }

    /// Get the first link between two nodes and remove it
    #[allow(dead_code)]
    pub fn unlink_nodes(
        &self,
        first_node_id: u32,
        second_node_id: u32,
    ) {
        self._raise_event(PipeWireEvent::UnlinkCommand(
            first_node_id,
            second_node_id,
        ));
        self.wait_for_event(|event: &ConnectorEvent| {
            *event
                == ConnectorEvent::UnlinkUpdate(
                    first_node_id,
                    second_node_id,
                )
                || *event
                    == ConnectorEvent::UnLinkFailed(
                        first_node_id,
                        second_node_id,
                    )
        });
    }

    fn wait_for_event<F: Fn(&ConnectorEvent) -> bool>(
        &self,
        checker: F,
    ) {
        let mut event_result: ConnectorEvent = ConnectorEvent::None;
        // Lock the thread and wait for the event to be processed
        while !checker(&event_result) {
            let result = self._receiver.try_recv();

            if let Err(e) = result {
                if e == TryRecvError::Disconnected {
                    log::error!("Failed to receive event: {e}");
                }
                continue;
            }
            event_result = result.unwrap();
        }
        log::debug!("(Connector) Received event: {event_result:?}")
    }

    pub fn get_objects(&self) -> Arc<RwLock<PipeWireObjects>> {
        self.objects.clone()
    }
}
