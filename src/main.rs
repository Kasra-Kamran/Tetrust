mod game_server;
use game_server::game_server_controller;

#[tokio::main]
async fn main()
{
    tokio::spawn(game_server_controller()).await.unwrap();
}
