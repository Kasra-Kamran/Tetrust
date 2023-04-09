mod webtetris;
mod comms;
use webtetris::WebTetris;
use tokio::{sync::{broadcast, mpsc}, net::{TcpListener, TcpStream}};
use comms::Comms;
use tokio_tungstenite::WebSocketStream;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use futures_util::{StreamExt, TryStreamExt, future::{self, Ready, BoxFuture, FutureExt}, SinkExt};
use serde::Deserialize;
use rand::{distributions::{Distribution, Uniform}, thread_rng};
use tungstenite::protocol::Message;

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

async fn game_server_basic(
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

async fn game_server(
    mut die: broadcast::Receiver<bool>,
    confirm_die: mpsc::Sender<bool>)
{
    let (confirm_game_end, mut kc_r) = mpsc::channel::<bool>(1);
    let (kill_games, mut die_games) = broadcast::channel::<bool>(1);
    let gamerooms: Arc<Mutex<Vec<GameRoom>>> = Arc::new(Mutex::new(Vec::new()));
    ////
    // fix the gameroom shit. this won't cut it.
    {
        let mut grs = gamerooms.lock().await;
        create_gameroom(3, &mut grs).await;
    }
    ////
    let listener = TcpListener::bind("0.0.0.0:12345").await.unwrap();
    tokio::select!
    {
        _ = die.recv() => {kill_games.send(true);}
        _ = async
        {
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
    // and all that fancy stuff, BUT, for later.

    kc_r.recv().await;
}

async fn handle_connection(
    cd: ConnectionData,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData)
{
    let ws_stream: WebSocketStream<TcpStream>;
    let username: String;
    if let Some(u) = cd.username
    {
        username = u;
        ws_stream = cd.ws_stream;
    }
    else if let Ok((u, ws)) = authenticate(cd.ws_stream).await
    {
        username = u;
        ws_stream = ws;
    }
    else
    {
        return;
    }
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
            tokio::spawn(perform_user_command(cd, user_msg, gamerooms, webtetris_data));
        },
    };
}

async fn perform_user_command(
    cd: ConnectionData,
    user_msg: UserMessage,
    gamerooms: Arc<Mutex<Vec<GameRoom>>>,
    webtetris_data: WebTetrisData,)
{
    match user_msg.command.as_str()
    {
        "join_game" => tokio::spawn(join_game(cd, user_msg, gamerooms, webtetris_data)),
        _ => todo!(),
    };
}

async fn listen_on_ws(ws_stream: WebSocketStream<TcpStream>) -> Result<(WebSocketStream<TcpStream>, UserMessage), ()>
{
    let (mut ws_outgoing, mut ws_incoming) = ws_stream.split();
    loop
    {   
        if let Some(msg) = ws_incoming.try_next().await.unwrap()
        {
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

async fn send_to_all(players: Vec<(String, WebSocketStream<TcpStream>)>, msg: String) -> Vec<(String, WebSocketStream<TcpStream>)>
{
    let num_players = players.len();
    let mut players2 = vec![];
    let (sender, mut receiver) = mpsc::channel::<(String, WebSocketStream<TcpStream>)>(3);
    for (username, ws_stream) in players
    {
        let msg2 = msg.clone();
        let sender_clone = sender.clone();
        tokio::spawn(async move
        {
            let (mut ws_outgoing, ws_incoming) = ws_stream.split();
            ws_outgoing.send(Message::binary(msg2)).await;
            let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
            sender_clone.send((username, ws_stream)).await;
        });
    }
    for _ in 0..num_players
    {
        if let Some(player) = receiver.recv().await
        {
            players2.push(player);
        }
    }
    players2
}

async fn receive_from_all(players: Vec<(String, WebSocketStream<TcpStream>)>, msg: String) -> Vec<(String, WebSocketStream<TcpStream>)>
{
    let num_players = players.len();
    let mut players2 = vec![];
    let (sender, mut receiver) = mpsc::channel::<Result<(String, WebSocketStream<TcpStream>), ()>>(3);
    for (username, ws_stream) in players
    {
        let msg2 = msg.clone();
        let sender_clone = sender.clone();
        tokio::spawn(async move
        {
            let (ws_outgoing, mut ws_incoming) = ws_stream.split();
            if let Some(msg) = ws_incoming.try_next().await.unwrap()
            {
                let msg = msg.to_string();
                loop
                {
                    if msg == msg2
                    {
                        break;
                    }
                }
                let ws_stream = ws_incoming.reunite(ws_outgoing).unwrap();
                sender_clone.send(Ok((username, ws_stream))).await;
            }
            else
            {
                sender_clone.send(Err(())).await;
            }
        });
    }
    for _ in 0..num_players
    {
        if let Some(Ok(player)) = receiver.recv().await
        {
            players2.push(player);
        }
    }
    players2
}

async fn authenticate(mut ws_stream: WebSocketStream<TcpStream>) -> Result<(String, WebSocketStream<TcpStream>), ()>
{
    let (mut ws_outgoing, mut ws_incoming) = ws_stream.split();
    let s: String;
    let user: User;
    if let Ok(Some(msg)) = ws_incoming.try_next().await
    {
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
            // let grs = gamerooms.clone();
            let mut gamerooms = gamerooms.lock().await;
            let mut room_index = 0;
            let mut selected = false;
            // it doesn't work!
            for (i, gameroom) in gamerooms.iter().enumerate()
            {
                if gameroom.capacity == capacity
                {
                    room_index = i;
                    selected = true;
                    break;
                }
            }
            if !selected
            {
                room_index = gamerooms.len();
                create_gameroom(capacity, &mut gamerooms).await;
            }
            let (gr, game_started) = add_to_gameroom_or_start(gamerooms.swap_remove(room_index), cd, wtd, sender).await;
            gamerooms.push(gr);
            if !game_started
            {
                return Ok(());
            }
        }
        for _ in 0..capacity
        {
            if let Some(Some((username, ws_stream))) = receiver.recv().await
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

async fn create_gameroom<'a>(capacity: u8, gamerooms: &mut MutexGuard<'a, Vec<GameRoom>>)
{
    let id;
    {
        let range = Uniform::from(0..1000);
        let mut rng = thread_rng();
        id = range.sample(&mut rng);
    }
    let gameroom = GameRoom
    {
        status: GameRoomStatus::Inactive,
        capacity: capacity,
        players: vec![],
        id,
    };
    gamerooms.push(gameroom);
}

async fn add_to_gameroom_or_start(mut gameroom: GameRoom, cd: ConnectionData, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>)>>) -> (GameRoom, bool)
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
        gameroom.players = send_to_all(gameroom.players, String::from("ready for game?")).await;
        if gameroom.capacity != <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
        {
            return (gameroom, false);
        }
        gameroom.players = receive_from_all(gameroom.players, String::from("ready!")).await;
        if gameroom.capacity != <usize as TryInto<u8>>::try_into(gameroom.players.len()).unwrap()
        {
            return (gameroom, false);
        }
        tokio::spawn(start_game(gameroom, webtetris_data, sender));
        return (new_gameroom, true);
    }
    return (gameroom, false);
}

async fn start_game(mut gameroom: GameRoom, webtetris_data: WebTetrisData, sender: mpsc::Sender<Option<(String, WebSocketStream<TcpStream>)>>)
{
    tokio::spawn(WebTetris::new(
        webtetris_data.dg,
        webtetris_data.cge,
        gameroom.players,
        sender));
}
