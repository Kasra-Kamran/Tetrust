mod webtetris;
mod comms;
use webtetris::WebTetris;
use tokio::{sync::{broadcast, mpsc}, net::{TcpListener, TcpStream}};
use comms::Comms;

async fn game_server(
    mut die: broadcast::Receiver<bool>,
    confirm_die: mpsc::Sender<bool>)
{
    let (confirm_game_end, mut kc_r) = mpsc::channel::<bool>(1);
    let (kill_games, mut die_games) = broadcast::channel::<bool>(1);
    let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();
    tokio::select!
    {
        _ = die.recv() => {kill_games.send(true);}
        _ = async
        {
            loop
            {
                let mut dg_clone = kill_games.subscribe();
                let cge_clone = confirm_game_end.clone();
                let mut players = vec![];
                for _ in 0..3
                {
                    let (stream, _) = listener.accept().await.unwrap();
                    let ws_stream = tokio_tungstenite::accept_async(stream)
                        .await
                        .unwrap();
                    players.push(ws_stream);
                }
                tokio::spawn(WebTetris::new(dg_clone, cge_clone, players));
            }
        } => {}
    };
    drop(confirm_game_end);
    kc_r.recv().await;
}

pub async fn game_server_controller()
{
    let (kill_game, mut die) = broadcast::channel::<bool>(1);
    let (kill_confirm, mut kc_r) = mpsc::channel::<bool>(1);
    tokio::spawn(game_server(die, kill_confirm));

    // control the game_server with a remote connection
    // and all that fancy stuff.

    let mut comms = Comms::new();
    comms.connect_to("127.0.0.1:8585").await;
    comms.send(String::from("hello there, ladies and gentlemen!")).await.unwrap();
    let mut s: String = comms.receive().await.unwrap();
    println!("{}", s);
    comms.send(String::from("well, hello there, fellow children!")).await.unwrap();
    s = comms.receive().await.unwrap();
    println!("{}", s);


    kc_r.recv().await;
}