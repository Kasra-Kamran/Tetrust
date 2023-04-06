mod webtetris;
mod comms;
use webtetris::WebTetris;
use tokio::{sync::{broadcast, mpsc}, net::{TcpListener, TcpStream}};
use comms::Comms;
use tokio_tungstenite::WebSocketStream;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

async fn game_server(
    mut die: broadcast::Receiver<bool>,
    confirm_die: mpsc::Sender<bool>)
{
    // let usernames = Vec::<String>::new();
    
    let (confirm_game_end, mut kc_r) = mpsc::channel::<bool>(1);
    let (kill_games, mut die_games) = broadcast::channel::<bool>(1);
    let list_of_ws: Arc<Mutex<HashMap<String, WebSocketStream<TcpStream>>>> =
        Arc::new(Mutex::new(HashMap::new()));
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
                let list_of_ws_clone = list_of_ws.clone();
                let mut players = vec![];
                for _ in 0..2
                {
                    let (stream, _) = listener.accept().await.unwrap();
                    let ws_stream = tokio_tungstenite::accept_async(stream)
                        .await
                        .unwrap();
                    
                    players.push(ws_stream);
                }
                tokio::spawn(WebTetris::new(dg_clone, cge_clone, players, list_of_ws_clone));
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

    // let mut comms = Comms::new();
    // comms.connect_to("127.0.0.1:8585").await;
    // comms.send(String::from("{\"command\":\"insert\", \"data\":\"message from rust!\", \"id\":85}")).await.unwrap();
    // let mut s: String = comms.receive().await.unwrap();
    // println!("{}", s);
    // comms.send(String::from("{\"command\":\"get\", \"id\":85}")).await.unwrap();
    // s = comms.receive().await.unwrap();
    // println!("{}", s);


    kc_r.recv().await;
}

/*
    websocket connection
    receive username and credentials
    communicate with django and authenticate
    if authenticated add to list_of_ws
    ------------------------------------
    # we need to actively listen on all websockets to see who wants to-
      queue a game.

    if anyone sends a message saying they want to play, we add them to-
     a queue of players who want to play(only their usernames, their websocket-
     connections should will go in list_of_ws).

    # the items of the queue are probably gonna be structs holding both the-
      players' usernames and the number of players they want to play with.

    another task manages the aforementioned queue and if there are enough people-
     it starts a game.

    the games end, and if the players hadn't disconnected,
     we add the websockets to list_of_ws, and restart the task that was
     listening on the websockets to wait for the players to either disconnect-
     or decide to join a game.
*/