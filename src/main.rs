use std::any::Any;
use std::env;
use std::io::Write;
use std::thread;
use std::time::Duration;

use serenity::all::{GetMessages, MessageInteractionMetadata, Timestamp, UserId};
use serenity::async_trait;
use serenity::model::channel::GuildChannel;
use serenity::model::channel::Message;
use serenity::model::gateway::Ready;
use serenity::prelude::*;

use chrono::offset::Local;
use serde::{Deserialize, Serialize};

struct Handler;

#[derive(Debug, Serialize, Deserialize)]
struct DiceRollInstance {
    number_of_dice: usize,
    size_of_dice: usize,
    modifier: isize,
    dice_rolls: Vec<usize>,
    total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct DiceRollRequest {
    rolls: Vec<DiceRollInstance>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OutData {
    message_id: u64,
    user_id: u64,
    unfiltered_contents: String,
    date: Timestamp,
    filterd_contents: Option<DiceRollRequest>,
}

enum Mode {
    GrabbingAndCleaning,
    Cleaning,
}
static MODE: Mode = Mode::GrabbingAndCleaning;

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, ctx: Context, msg: Message) {
        if msg.content == "~!get_data" {
            if let Err(why) = msg
                .channel_id
                .say(&ctx.http, "Getting Data, and then cleaning it")
                .await
            {
                println!("Error sending message: {why:?}");
            }

            let mut cleaned_messages = match MODE {
                Mode::GrabbingAndCleaning => {
                    let channel = msg.channel_id.to_channel(&ctx.http).await.unwrap();

                    let (messages, amount_read) =
                        dbg!(self.get_messages(&ctx, channel.guild().unwrap()).await);

                    if let Err(why) = msg
                        .channel_id
                        .say(
                            &ctx.http,
                            format!(
                                "Read {amount_read} messages, and {0} were from the bot",
                                messages.len()
                            ),
                        )
                        .await
                    {
                        println!("Error sending message: {why:?}");
                    }

                    let cleaned_messages: Vec<OutData> = messages
                        .iter()
                        .map(|m| OutData {
                            message_id: m.id.get(),
                            user_id: m.interaction.as_ref().unwrap().user.id.get(),
                            unfiltered_contents: m.content.clone(),
                            date: m.timestamp,
                            filterd_contents: None,
                        })
                        .collect();
                    let mut file =
                        std::fs::File::create(format!("unfiltered_{}.json", Local::now())).unwrap();
                    let _ = file
                        .write_all(serde_json::to_string(&cleaned_messages).unwrap().as_bytes());
                    cleaned_messages
                }
                Mode::Cleaning => {
                    let file = std::fs::read_to_string("unfiltered_1.json").unwrap();
                    return serde_json::from_str(&file).unwrap();
                }
            };

            cleaned_messages.iter_mut().for_each(|m| {
                m.filterd_contents = parser::parse_roll(m.unfiltered_contents.clone())
            });

            let mut file = std::fs::File::create(format!("cleaned_{}.json", Local::now())).unwrap();
            let _ = file.write_all(serde_json::to_string(&cleaned_messages).unwrap().as_bytes());
        }
    }

    async fn ready(&self, _: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
    }
}

impl Handler {
    async fn get_messages(&self, ctx: &Context, channel: GuildChannel) -> (Vec<Message>, usize) {
        let mut dice_rolls: Vec<Message> = Vec::new();
        let bot_id = UserId::new(809017610111942686);
        let mut target_message = channel.last_message_id.unwrap();
        let mut total_message_count = 0;
        loop {
            // grab 100 messages
            let resp = channel
                .messages(
                    &ctx.http,
                    GetMessages::new().before(target_message).limit(100),
                )
                .await
                .unwrap();
            if resp.is_empty() {
                break;
            }
            total_message_count += resp.len();
            target_message = resp.last().unwrap().id;
            let mut bot_messages: Vec<Message> = resp
                .iter()
                .filter(|m| m.author.id == bot_id)
                .cloned()
                .collect();
            dice_rolls.append(&mut bot_messages);
            thread::sleep(Duration::from_millis(10));
        }
        (dice_rolls, total_message_count)
    }
}

#[tokio::main]
async fn main() {
    let token =
        env::var("DISCORD_TOKEN").expect("Expected a token, please update your environment");
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;
    let mut client = Client::builder(&token, intents)
        .event_handler(Handler)
        .await
        .expect("Err creating client");
    if let Err(why) = client.start().await {
        println!("Client error: {why:?}");
    }
}
mod parser {
    use crate::{DiceRollInstance, DiceRollRequest};
    use winnow::{
        Parser, Result,
        ascii::{digit1, multispace0, multispace1},
        combinator::{alt, delimited, opt, separated, separated_pair},
        token::{take, take_until},
    };

    pub fn parse_roll(input: String) -> Option<DiceRollRequest> {
        ("```", separated(.., parse_table, "``````"))
            .parse_next(&mut &input[..])
            .map(|(_, v)| DiceRollRequest { rolls: v })
            .ok()
    }

    fn parse_table(input: &mut &str) -> Result<DiceRollInstance> {
        (
            parse_table_top,
            parse_dice_input,
            parse_table_middle,
            parse_dice_output,
            parse_table_bottom,
        )
            .parse_next(input)
            .map(|(_, i, _, o, _)| {
                let mut out = o.0.clone();
                if i.2 != 0 {
                    _ = out.remove(out.len() - 1)
                }
                DiceRollInstance {
                    number_of_dice: i.0,
                    size_of_dice: i.1,
                    modifier: i.2,
                    dice_rolls: out,
                    total: o.1,
                }
            })
    }

    fn parse_table_top(input: &mut &str) -> Result<()> {
        (take_until(.., '║'), take(1usize)).void().parse_next(input)
    }

    fn parse_table_middle(input: &mut &str) -> Result<()> {
        (
            take_until(.., '┼'),
            take_until(.., '║'),
            take_until(.., '║'),
            take(1usize),
        )
            .void()
            .parse_next(input)
    }

    fn parse_table_bottom(input: &mut &str) -> Result<()> {
        take_until(.., '`').void().parse_next(input)
    }

    fn parse_dice_input(input: &mut &str) -> Result<(usize, usize, isize)> {
        delimited(
            multispace0,
            (
                digit1.parse_to(),
                'd',
                digit1.parse_to(),
                opt((alt(('-', '+')), digit1.parse_to::<isize>())),
            ),
            multispace0,
        )
        .parse_next(input)
        .map(|(num, _, size, modifier)| {
            (
                num,
                size,
                modifier
                    .map(|(sign, val)| match sign {
                        '-' => 0 - val,
                        '+' => val,
                        _ => unreachable!("Matched a char which it shouldn't have"),
                    })
                    .unwrap_or_default(),
            )
        })
    }

    fn parse_dice_output(input: &mut &str) -> Result<(Vec<usize>, usize)> {
        separated_pair(
            delimited(
                multispace0,
                separated(.., digit1.parse_to::<usize>(), multispace1),
                multispace0,
            ),
            '│',
            delimited(
                multispace0,
                delimited('[', digit1.parse_to(), ']'),
                multispace0,
            ),
        )
        .parse_next(input)
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn total_example() {
            let input = "```╔═════════════════╗\n║    1d10     ║\n╠══════════╤══════╣\n║  rolls   │ sum  ║\n╟──────────┼──────╢\n║  6 3 2   │ [11] ║\n╚══════════╧══════╝```";
            assert!(dbg!(parse_roll(input.to_string())).is_some());
            let input2 = "```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║ 5 10 20  │ [35] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  5 5 20  │ [30] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  3 2 20  │ [25] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  9 4 20  │ [33] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  2 9 20  │ [31] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  6 2 20  │ [28] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  9 7 20  │ [36] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║ 10 6 20  │ [36] ║
╚══════════╧══════╝```
```╔═════════════════╗
║     2d10+20     ║
╠══════════╤══════╣
║  rolls   │ sum  ║
╟──────────┼──────╢
║  3 7 20  │ [30] ║
╚══════════╧══════╝```";
            assert!(dbg!(parse_roll(input2.to_string())).is_none());
        }
    }
}
