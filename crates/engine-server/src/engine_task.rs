// crates/engine-server/src/engine_task.rs

use tokio::sync::mpsc::UnboundedReceiver;
use engine_core::{MatchingEngine, OutputMessage};
use crate::types::{ClientRegistry, EngineRequest};

pub async fn run_engine_loop(
    mut engine_rx: UnboundedReceiver<EngineRequest>,
    clients: ClientRegistry,
) {
    let mut engine = MatchingEngine::new();
    let mut requests_received: u64 = 0;
    let mut outputs_generated: u64 = 0;
    
    eprintln!("Engine task: started");
    
    while let Some(EngineRequest { client_id, msg }) = engine_rx.recv().await {
        requests_received += 1;
        
        eprintln!("Engine: Processing {:?} from client {}", msg, client_id.0);
        
        // Process message in the matching engine
        let outputs: Vec<OutputMessage> = engine.process_message(msg);
        outputs_generated += outputs.len() as u64;
        
        eprintln!("Engine: Generated {} outputs", outputs.len());
        for out in &outputs {
            eprintln!("  -> {:?}", out);
        }
        
        // Broadcast to all clients
        let guard = clients.read().await;
        for (target_id, tx) in guard.iter() {
            for out in &outputs {
                if tx.send(out.clone()).is_err() {
                    eprintln!("Failed to send to client {}", target_id.0);
                }
            }
        }
    }
    
    eprintln!("==============================================================");
    eprintln!("Engine task shutting down.");
    eprintln!("  Requests received:  {}", requests_received);
    eprintln!("  Outputs generated:  {}", outputs_generated);
    eprintln!("==============================================================");
}
