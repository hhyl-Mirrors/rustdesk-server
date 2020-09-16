use hbb_common::{
    env_logger, log,
    protobuf::Message as _,
    rendezvous_proto::*,
    sleep,
    tcp::{new_listener, FramedStream},
    tokio, ResultType,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

lazy_static::lazy_static! {
    static ref PEERS: Arc<Mutex<HashMap<String, FramedStream>>> = Arc::new(Mutex::new(HashMap::new()));
}

#[tokio::main]
async fn main() -> ResultType<()> {
    env_logger::init();
    let addr = "0.0.0.0:21117";
    log::info!("Listening on {}", addr);
    let mut listener = new_listener(addr, true).await?;
    loop {
        tokio::select! {
            Ok((stream, addr)) = listener.accept() => {
                tokio::spawn(async move {
                    make_pair(FramedStream::from(stream), addr).await.ok();
                });
            }
        }
    }
}

async fn make_pair(stream: FramedStream, addr: SocketAddr) -> ResultType<()> {
    let mut stream = stream;
    if let Some(Ok(bytes)) = stream.next_timeout(30_000).await {
        if let Ok(msg_in) = RendezvousMessage::parse_from_bytes(&bytes) {
            if let Some(rendezvous_message::Union::request_forward(rf)) = msg_in.union {
                if !rf.uuid.is_empty() {
                    let peer = PEERS.lock().unwrap().remove(&rf.uuid);
                    if let Some(peer) = peer {
                        log::info!("Forward request {} from {} got paired", rf.uuid, addr);
                        return forward(stream, peer).await;
                    } else {
                        log::info!("New forward request {} from {}", rf.uuid, addr);
                        PEERS.lock().unwrap().insert(rf.uuid.clone(), stream);
                        sleep(30.).await;
                        PEERS.lock().unwrap().remove(&rf.uuid);
                    }
                }
            }
        }
    }
    Ok(())
}

async fn forward(stream: FramedStream, peer: FramedStream) -> ResultType<()> {
    let mut peer = peer;
    let mut stream = stream;
    peer.set_raw();
    stream.set_raw();
    loop {
        tokio::select! {
            res = peer.next() => {
                if let Some(Ok(bytes)) = res {
                    stream.send_bytes(bytes.into()).await?;
                } else {
                    break;
                }
            },
            res = stream.next() => {
                if let Some(Ok(bytes)) = res {
                    peer.send_bytes(bytes.into()).await?;
                } else {
                    break;
                }
            },
        }
    }
    Ok(())
}