use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
use futures_channel::mpsc::{unbounded, UnboundedSender};
use tokio::{io::AsyncReadExt, time::sleep};
use futures_util::{future, pin_mut, StreamExt};
use std::time::Duration;
use std::io;

#[tokio::main]
async fn main()
{
    let stdin = io::stdin();
    let mut asdf = String::new();
    for i in 0..30
    {
        let url = url::Url::parse("ws://127.0.0.1:12345/").unwrap();
        let (ws_stream, _) = connect_async(url).await.unwrap();
        println!("made connection: {}", i);
        let (tx, rx) = unbounded();
        let (transmitter, receiver) = ws_stream.split();
        let rx_to_receiver = rx.map(Ok).forward(transmitter);
        tokio::spawn(rx_to_receiver);

        // stdin.read_line(&mut asdf);
        sleep(Duration::from_millis(100));
        tx.unbounded_send(Message::binary("end_game")).unwrap();
        println!("ended game");
        sleep(Duration::from_millis(300)).await;
    }
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