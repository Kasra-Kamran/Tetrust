use tokio::net::{TcpListener, TcpStream};
use futures_channel::mpsc::unbounded;
use futures_util::{future, pin_mut, StreamExt, TryStreamExt};

#[tokio::main]
async fn main()
{
    let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    tokio::spawn(handle_connection(stream)).await;
}

async fn handle_connection(stream: TcpStream)
{
    let ws_stream = tokio_tungstenite::accept_async(stream)
        .await
        .unwrap();
    let (tx, rx) = unbounded();
    let (transmitter, receiver) = ws_stream.split();

    let ws_to_tx = receiver.try_for_each(|msg|
    {
        println!("{}", msg.to_text().unwrap());
        tx.unbounded_send(msg.clone()).unwrap();
        future::ok(())
    });

    let rx_to_ws = rx.map(Ok).forward(transmitter);

    pin_mut!(rx_to_ws, ws_to_tx);
    future::select(rx_to_ws, ws_to_tx).await;

}