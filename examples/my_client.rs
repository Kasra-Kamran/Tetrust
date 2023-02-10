use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use tokio::io::AsyncReadExt;
use futures_util::{future, pin_mut, StreamExt};

#[tokio::main]
async fn main()
{
    let url = url::Url::parse("ws://127.0.0.1:12345/").unwrap();
    let (ws_stream, _) = connect_async(url).await.unwrap();
    
    let (tx, rx) = unbounded();
    let (transmitter, receiver) = ws_stream.split();

    let stdin_to_tx = tokio::spawn(read_stdin(tx));
    let rx_to_receiver = rx.map(Ok).forward(transmitter);

    pin_mut!(stdin_to_tx, rx_to_receiver);
    future::select(stdin_to_tx, rx_to_receiver).await;
}

async fn read_stdin(tx: UnboundedSender<Message>)
{
    let mut stdin = tokio::io::stdin();
    loop {
        let mut buf = vec![0; 1024];
        let n = match stdin.read(&mut buf).await {
            Err(_) | Ok(0) => break,
            Ok(n) => n,
        };
        buf.truncate(n);
        tx.unbounded_send(Message::binary(buf)).unwrap();
    }
}