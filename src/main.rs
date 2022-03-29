use dominion::prelude::*;
use dominion_server::prelude::*;

use std::sync::{Arc, Mutex};

use anyhow::Result;
use futures::prelude::*;
use tokio::net::TcpStream;
use tokio_serde::formats::*;
use tokio_util::codec::{FramedRead, FramedWrite, LengthDelimitedCodec};

enum InputMode {
    Console,
    Chat,
}

#[tokio::main]
pub async fn main() -> Result<()> {
    let socket = TcpStream::connect("localhost:31194").await?;

    // Duplicate the socket: one for serializing and one for deserializing
    let socket = socket.into_std()?;
    let socket2 = socket.try_clone()?;
    let socket = TcpStream::from_std(socket)?;
    let socket2 = TcpStream::from_std(socket2)?;

    let length_delimited = FramedRead::new(socket, LengthDelimitedCodec::new());
    let mut deserialized: ServerMessageReceiver =
        tokio_serde::SymmetricallyFramed::new(
            length_delimited,
            SymmetricalJson::<ServerMessage>::default(),
        );

    let length_delimited = FramedWrite::new(socket2, LengthDelimitedCodec::new());
    let mut serialized: ValueSender =
        tokio_serde::SymmetricallyFramed::new(
            length_delimited,
            SymmetricalJson::default()
        );

    let game_state = Arc::new(Mutex::new(PartialGame::default()));
    let game_state2 = game_state.clone();

    // Handle incoming messages from the server
    tokio::spawn(async move {
        while let Some(msg) = deserialized.try_next().await.unwrap() {
            match msg {
                ServerMessage::PingResponse => {
                    println!("pong!");
                }
                ServerMessage::ChatMessage{ author, message } => {
                    println!("Player {}: \"{}\"", author, message)
                }
                ServerMessage::StartingGame{ state } => {
                    let mut old_state = game_state.lock().unwrap();
                    *old_state = state;
                    println!("Starting game!");
                }
                ServerMessage::NotEnoughPlayers => {
                    println!("Not enough players to start!");
                }
                ServerMessage::CurrentState{ state } => {
                    let mut old_state = game_state.lock().unwrap();
                    *old_state = state;
                }
                ServerMessage::IllegalPlay { card, reason } => {
                    match reason {
                        DominionError::WrongPhase => println!("You can't do that during this phase"),
                        _ => println!("You can't play {} here", card),
                    }
                }
                _ => {
                    println!("Got a message from the server that the client couldn't understand!")
                }
            }
        }
    });

    let mut input_mode = InputMode::Console;

    // Continuously read user input and send appropriate messages to the server
    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).expect("Error while reading user input!");
        let trimmed = input.trim_matches(char::is_whitespace);
        match input_mode {
            InputMode::Console => {
                let mut command_parts = trimmed.split_whitespace();
                match command_parts.next().unwrap_or("Oops") {
                    "ping" => {
                        serialized
                        .send(serde_json::to_value(&ClientMessage::Ping)?)
                        .await?;
                    }
                    "chat" => {
                        input_mode = InputMode::Chat;
                    }
                    "quit" => {
                        panic!("Exiting client")
                    }
                    "start" => {
                        serialized
                        .send(serde_json::to_value(&ClientMessage::StartGame { supply_list: Game::default_supply_list() })?)
                        .await?;
                    }
                    "hand" => {
                        let state = game_state2.lock().unwrap();
                        let hand: Vec<Box<dyn Card>> = state.player.hand.clone().into();
                        let names: Vec<String> = hand.iter().map(|card| card.name().to_string()).collect();
                        println!("{}", names.join(", "));
                    }
                    "play" => {
                        let card_name = command_parts.next().unwrap_or("");
                        let state = game_state2.lock().unwrap();
                        let hand = &state.player.hand;
                        let n = hand
                                                    .clone()
                                                    .into_iter()
                                                    .position(|c| c.clean_name() == card_name);
                        match n {
                            Some(index) => {
                                serialized
                                    .send(serde_json::to_value(&ClientMessage::PlayCard { index })?)
                                    .await?;
                            }
                            None => {
                                println!("Couldn't find any card named {} in hand!", card_name)
                            }
                        }
                    }
                    "pass" => {
                        let state = game_state2.lock().unwrap();
                        serialized
                            .send(serde_json::to_value(&ClientMessage::NextPhase)?)
                            .await?;
                    }
                    _ => println!("Couldn't understand input!")
                }
            }
            InputMode::Chat => {
                match trimmed {
                    "" => {}
                    "/exit" => {
                        println!("Leaving chat mode!");
                        input_mode = InputMode::Console;
                    }
                    _ => {
                        serialized
                        .send(serde_json::to_value(&ClientMessage::ChatMessage{ message: trimmed.to_string() })?)
                        .await?;
                    }
                }
            }
        }

    }
}
