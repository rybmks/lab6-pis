use axum::{
    Router,
    extract::{
        Extension,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use futures::{sink::SinkExt, stream::StreamExt};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

type Tx = UnboundedSender<String>;
type Clients = Arc<Mutex<Vec<Tx>>>;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let clients: Clients = Arc::new(Mutex::new(Vec::new()));

    let app = Router::new()
        .route("/", get(serve_html))
        .route("/ws", get(handle_socket))
        .layer(Extension(clients.clone()));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Server started at http://{}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

async fn serve_html() -> impl IntoResponse {
    tracing::info!("Rendering html client");

    Html(
        tokio::fs::read_to_string("index.html")
            .await
            .expect("Failed to read index.html"),
    )
}

async fn handle_socket(
    ws: WebSocketUpgrade,
    Extension(clients): Extension<Clients>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_connection(socket, clients))
}

async fn handle_connection(socket: WebSocket, clients: Clients) {
    tracing::info!("New WebSocket connection");

    let (mut sender_ws, mut receiver_ws) = socket.split();
    let (tx, mut rx) = unbounded_channel::<String>();

    {
        clients.lock().unwrap().push(tx.clone());
    }

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender_ws.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    let recv_task = {
        let clients = clients.clone();
        tokio::spawn(async move {
            while let Some(Ok(Message::Text(text))) = receiver_ws.next().await {
                tracing::info!("Received: {}", text);

                let clients_guard = clients.lock().unwrap();
                for client in clients_guard.iter() {
                    let _ = client.send(text.to_string());
                }
            }
        })
    };

    let _ = tokio::join!(send_task, recv_task);

    clients
        .lock()
        .unwrap()
        .retain(|client| !client.same_channel(&tx));
    tracing::info!("Client disconnected");
}
