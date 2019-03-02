mod cli;
mod core;
mod db;
mod bgg;
mod lib;

use crate::core::Message;
use cli::Cli;
use structopt::StructOpt;
use failure::Error;
use exitfailure::ExitFailure;
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::io::Write;
use ctrlc;

fn main() -> Result<(), ExitFailure> {
    let cli = Cli::from_args();
    match cli {
        Cli::New { } => create_structure()?,
        Cli::Report { } => make_report()?,
        Cli::Pull { } => pull_games()?,
        Cli::Balance { } => stabilize()?,
        Cli::Review { } => review_users()?
    }
    Ok(())
}

fn create_structure() -> Result<(), Error> {
    core::create_structure()?;
    println!("Created initial structure files.");
    Ok(())
}

fn make_report() -> Result<(), Error> {
    let games = core::make_report()?;
    if games.is_empty() {
        println!("Game list is not stable enough.");
    } else {
        println!("Id\tName\tRating\tVotes\tGeek Rating\tAvg BGG Rating\tBGG Votes");
        for game in games {
            println!("{}\t{}\t{:.2}\t{}\t{}\t{}\t{}",
                game.id, game.name, game.rating, game.votes,
                game.bgg_geek_rating, game.bgg_avg_rating, game.bgg_num_votes);
        }
    }
    Ok(())
}

fn pull_games() -> Result<(), Error> {
    let config = core::config()?;
    println!("Starting download.");
    core::pull_games(config.limit, |i| {
        println!("Downloaded page: {}", i);
    })?;
    println!("Finished download.");
    Ok(())
}

fn stabilize() -> Result<(), Error> {
    // // Cancellation token
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    // Bind cancellation token with ctrl+c command
    ctrlc::set_handler(move || {
         r.store(false, Ordering::SeqCst);
    })?;
    // Load config
    let config = core::config()?;
    println!("Start balancing.");
    // Prettify output a bit
    let mut stdout = StandardStream::stdout(ColorChoice::Always);
    let mut seen_users: u32 = 0;
    let mut requests: u32 = 0;
    let mut balanced_games: u32 = 0;
    let mut num_errs: u32 = 0;
    core::stabilize(config, running, |m| match m {
        Message::NoteUserProgress(_) => {
            seen_users += 1;
            if seen_users % 50 == 0 {
                stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green))).unwrap();
                writeln!(&mut stdout, "Found another 50.").unwrap();
            };
        },
        Message::DieResult(game) => {
            balanced_games += 1;
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Yellow))).unwrap();
            writeln!(&mut stdout, "{} is balanced.", game.name).unwrap();
        },
        Message::NoteErr(error) => {
            num_errs += 1;
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Red))).unwrap();
            writeln!(&mut stdout, "{:?}", error).unwrap();
        },
        Message::NoteGameProgress(game) => {
            requests += 1;
            stdout.set_color(ColorSpec::new().set_fg(Some(Color::Green))).unwrap();
            writeln!(&mut stdout, "About to ask BGG about {}", game.name).unwrap();
        },
        _ => {} 
    })?;
    println!("Seen {} users, {} balanced games, {} erorrs, {} game requests.",
        seen_users, balanced_games, num_errs, requests);
    println!("Finished balancing.");
    Ok(())
}

fn review_users() -> Result<(), Error> {
    // TODO: make unstable again. trusted after 180 untrusted 90
    // any update on user in that mode
    // makes gametable unbalanced
    Ok(())
}
