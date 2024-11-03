use tokio::sync::mpsc::Sender;
use almost_raft::{Message, Node};
use async_trait::async_trait;


#[derive(Debug, Clone)]
struct NodeMPSC {
    id: String,
    sender: Sender<Message<NodeMPSC>>,
}

#[async_trait]
impl Node for NodeMPSC {
    type NodeType = NodeMPSC;
    async fn send_message<'a>(&'a self, msg: Message<Self::NodeType>) {
        let _ = self.sender.send(msg).await;
    }

    fn node_id(&self) -> &String {
        &self.id
    }
    
}