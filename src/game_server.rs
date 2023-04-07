mod webtetris;
mod comms;
use webtetris::WebTetris;
use tokio::{sync::{broadcast, mpsc}, net::{TcpListener, TcpStream}};
use comms::Comms;
use tokio_tungstenite::WebSocketStream;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use futures_util::{StreamExt, TryStreamExt, future::{self, Ready, BoxFuture, FutureExt}, SinkExt};
use serde::Deserialize;
use rand::{distributions::{Distribution, Uniform}, thread_rng};

#[derive(Deserialize)]
struct User
{
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct UserMessage
{
    command: String,
    gameroom_capacity: Option<u8>,
}

struct ConnectionData
{
    ws_stream: WebSocketStream<TcpStream>,
    username: Option<String>,
}

#[derive(std::fmt::Debug)]
enum GameRoomStatus
{
    Inactive,
    Active,
}

#[derive(std::fmt::Debug)]
struct GameRoom
{
    capacity: u8,
    status: GameRoomStatus,
    players: Vec<(String, WebSocketStream<TcpStream>)>,
    id: u16,
}

struct WebTetrisData
{
    dg: broadcast::Receiver<bool>,
    cge: mpsc::Sender<bool>,
    kill_games: Option<broadcast::Sender<bool>>,
}

async fn game_server(
    mut die: broadcast::Receiver<bool>,
    confirm_die: mpsc::Sender<bool>)
{   
    let (confirm_game_end, mut kc_r) = mpsc::channel::<bool>(1);
    let (kill_games, mut die_games) = broadcast::channel::<bool>(1);
    let (sender, _) = mpsc::channel::<Option<(String, WebSocketStream<TcpStream>)>>(2);
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
                let sender_clone = sender.clone();
                let mut players = vec![];
                for _ in 0..2
                {
                    let (stream, _) = listener.accept().await.unwrap();
                    let ws_stream = tokio_tungstenite::accept_async(stream)
                        .await
                        .unwrap();
                    
                    players.push((String::from("it's a me, mario!"), ws_stream));
                }
                tokio::spawn(WebTetris::new(dg_clone, cge_clone, players, sender_clone));
            }
        } => {}
    };
    drop(confirm_game_end);
    kc_r.recv().await;
}

pub async fn game_server_controller()
{
    // let (kill_game, mut die) = broadcast::channel::<bool>(1);
    // let (kill_confirm, mut kc_r) = mpsc::channel::<bool>(1);
    // tokio::spawn(game_server(die, kill_confirm));

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

    /////////////////////////////////
    let (confirm_game_end, mut kc_r) = mpsc::channel::<bool>(1);
    let (kill_games, mut die_games) = broadcast::channel::<bool>(1);
    let gamerooms: Arc<Mutex<Vec<GameRoom>>> = Arc::new(Mutex::new(Vec::new()));
    ///////////////////////
    let grs = gamerooms.clone();
    create_gameroom(2, grs);
    ///////////////////////
    let listener = TcpListener::bind("127.0.0.1:12345").await.unwrap();
    loop
    {
        let mut dg_clone = kill_games.subscribe();
        let cge_clone = confirm_game_end.clone();
        let gamerooms_clone = gamerooms.clone();
        let kill_games_clone = kill_games.clone();

        let webtetris_data = WebTetrisData
        {
            dg: dg_clone,
            cge: cge_clone,
            kill_games: Some(kill_games_clone),
        };

        let (stream, _) = listener.accept().await.unwrap();
        let ws_stream = tokio_tungstenite::accept_async(stream)
            .await
            .unwrap();
        let cd = ConnectionData
        {
            ws_stream,
            username: None,
        };
        tokio::spawn(handle_connection(cd, gamerooms_clone, webtetris_data));
    }
    drop(confirm_game_end);
    kc_r.recv().await;
    /////////////////////////////////

    // kc_r.recv().await;
}

async fn handle_connection(
    cd: ConnectionData,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData)
{
    // figure out a way to make this DRY.
    if let Some(u) = cd.username
    {
        match listen_on_ws(cd.ws_stream).await
        {
            Err(_) => return,
            Ok((ws_stream, user_msg)) =>
            {
                let cd = ConnectionData
                {
                    ws_stream,
                    username: Some(u),
                };
                match user_msg.command.as_str()
                {
                    "join_game" => tokio::spawn(join_game(cd, user_msg, gamerooms, webtetris_data)),
                    _ => todo!(),
                };
            },
        };
    }
    else
    {
        if let Ok((username, ws_stream)) = authenticate(cd.ws_stream).await
        {
            match listen_on_ws(ws_stream).await
            {
                Err(_) => return,
                Ok((ws_stream, user_msg)) =>
                {
                    let cd = ConnectionData
                    {
                        ws_stream,
                        username: Some(username),
                    };
                    match user_msg.command.as_str()
                    {
                        "join_game" => tokio::spawn(join_game(cd, user_msg, gamerooms, webtetris_data)),
                        _ => todo!(),
                    };
                },
            };
        }
    }
}

fn create_gameroom(capacity: u8, gamerooms: Arc<Mutex<Vec<GameRoom>>>)
{
    let range = Uniform::from(0..1000);
    let mut rng = thread_rng();
    let id = range.sample(&mut rng);
    let gameroom = GameRoom
    {
        status: GameRoomStatus::Inactive,
        capacity: capacity,
        players: vec![],
        id,
    };
    let mut gamerooms = gamerooms.lock().unwrap();
    gamerooms.push(gameroom);
}

fn add_to_gameroom_or_start(mut gameroom: GameRoom, cd: ConnectionData, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>)>>) -> (GameRoom, bool)
{
    let new_gameroom = GameRoom
    {
        players: vec![],
        capacity: gameroom.capacity,
        status: GameRoomStatus::Inactive,
        id: gameroom.id,
    };
    if let Some(username) = cd.username
    {
        gameroom.players.push((username, cd.ws_stream));
    }
    if gameroom.capacity == <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
    {
        start_game(gameroom, webtetris_data, sender);
        return (new_gameroom, true);
    }
    return (gameroom, false);
}

fn start_game(gameroom: GameRoom, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>)>>)
{
    tokio::spawn(WebTetris::new(
        webtetris_data.dg,
        webtetris_data.cge,
        gameroom.players,
        sender));
}

fn join_game(
    cd: ConnectionData,
    user_msg: UserMessage,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData) -> BoxFuture<'static, Result<(), ()>>
{
    async move
    {   
        let capacity: u8;
        let capacity = user_msg.gameroom_capacity.ok_or(())?;
        let (sender, mut receiver) = mpsc::channel::<Option<(String, WebSocketStream<TcpStream>)>>(2);
        let (confirm_games_end, mut kc_r) = mpsc::channel::<bool>(1);
        let kill_games: broadcast::Sender<bool> = webtetris_data
            .kill_games
            .ok_or(())?;
        let wtd = WebTetrisData
        {
            dg: kill_games.subscribe(),
            cge: confirm_games_end,
            kill_games: None,
        };
        {
            // what if there are no rooms with that many players?
            // you just gonna use room 0?!
            let mut gamerooms = gamerooms.lock().unwrap();
            let mut room_index = 0;
            for (i, gameroom) in gamerooms.iter().enumerate()
            {
                if gameroom.capacity == capacity
                {
                    room_index = i;
                    break;
                }
            }
            let (gr, game_started) = add_to_gameroom_or_start(gamerooms.swap_remove(room_index), cd, wtd, sender);
            gamerooms.push(gr);
            if !game_started
            {
                return Ok(());
            }
        }
        for _ in 0..capacity
        {
            if let Some((username, ws_stream)) = receiver.recv().await.unwrap()
            {
                let cd = ConnectionData
                {
                    ws_stream,
                    username: Some(username),
                };
                let kill_games_clone = kill_games.clone();
                let wtd = WebTetrisData
                {
                    dg: kill_games.subscribe(),
                    cge: webtetris_data.cge.clone(),
                    kill_games: Some(kill_games_clone),
                };
                let gamerooms_clone = gamerooms.clone();
                tokio::spawn(handle_connection(cd, gamerooms_clone, wtd));
            }
        }
        kc_r.recv().await;
        Ok(())
    }.boxed()
}

async fn listen_on_ws(ws_stream: WebSocketStream<TcpStream>) -> Result<(WebSocketStream<TcpStream>, UserMessage), ()>
{
    let (mut ws_outgoing, mut ws_incoming) = ws_stream.split();
    loop
    {   
        if let Some(msg) = ws_incoming.try_next().await.unwrap()
        {
            println!("{}", msg.to_string());
            let msg = msg.to_string();
            if let Ok(user_msg) = serde_json::from_str(&msg)
            {
                let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                return Ok((ws_stream, user_msg));
            }
        }
        else
        {
            return Err(());
        }
    }
}

async fn authenticate(mut ws_stream: WebSocketStream<TcpStream>) -> Result<(String, WebSocketStream<TcpStream>), ()>
{
    let (mut ws_outgoing, mut ws_incoming) = ws_stream.split();
    let s: String;
    let user: User;
    if let Some(msg) = ws_incoming.try_next().await.unwrap()
    {
        println!("{}", msg.to_string());
        user = serde_json::from_str(&msg.to_string()).unwrap();
        let mut comms: Comms = Comms::new();
        comms.connect_to("127.0.0.1:8585").await;
        comms.send(String::from(msg.to_string())).await.unwrap();
        s = comms.receive().await.unwrap();
    }
    else
    {
        return Err(());
    }
    if s == "not_authentic"
    {
        return Err(());
    }
    ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
    Ok((user.username, ws_stream))
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