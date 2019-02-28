use crate::db;
use crate::bgg;
use crate::lib::{Game, User};
use failure::{Error, ResultExt, ensure};
use std::fs;
use serde_json::{from_str, to_string_pretty};
use serde_derive::{Serialize, Deserialize};
use std::thread;
use std::sync::mpsc;
use std::sync::mpsc::Sender;
use std::time::Duration;
use reqwest::Client;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use threadpool::ThreadPool;
use std::collections::HashMap;

const CONFIG_FILE_NAME: &str = "app.config";
const LOWER_BOUND: f64 = 2.0;
const UPPER_BOUND: f64 = 8.0;

pub fn create_structure() -> Result<(), Error> {
    // create config file
    let new_conf = to_string_pretty(&Config::new(1000, 20, 500, 4))?;
    fs::write(CONFIG_FILE_NAME, new_conf)?;
    // create db file
    db::initialize()?;
    Ok(())
}

pub fn pull_games(limit: u32, progress: impl Fn(usize) -> ()) -> Result<(), Error> {
    ensure!(limit > 0, "Can't get top.");

    // clear db
    db::drop_all_games()?;
    // Collect games
    for (i, games) in bgg::GameIterator::new(&Client::new(), limit).enumerate() {
        // Error will be elevated and next() will be never called again
        let games_on_page = games?;
        db::add_games(games_on_page)?;
        progress(i + 1);
    }
    Ok(())
}

pub fn make_report() -> Result<Vec<Game>, Error> {
    let conn = db::DbConn::new()?;
    if conn.get_number_of_unstable_games()? == 0 {
        conn.get_all_games()
    } else {
        Ok(Vec::new())
    }
}

fn trust(rating: f64) -> bool {
    LOWER_BOUND < rating && rating < UPPER_BOUND
}

/// Err => Unrecoverable error, no signal sent
/// None => bgg is busy, must ask again later
/// Hashmap => got info on every user
fn check_users<'a>(tx: &Sender<Message>, conn: &db::DbConn, client: &Client, tkn: &mut RegulationToken,
        users: &'a [(User, f64)]) -> Result<Option<HashMap<&'a User, bool>>, Error> {
    
    let mut user_map: HashMap<&User, bool> = HashMap::new();
    for (user, _) in users {
        // check if we have seen user already
        match conn.check_user(&user) {
            // see him first time
            Ok(None) => {
                // ask bgg for user stats
                let rating = match bgg::get_user_average_rating(client, &user) {
                    Err(e) => {
                        tx.send(Message::NoteErr(e)).unwrap();
                        tkn.harden(); // wait a bit longer before next request
                        return Ok(None);
                    },
                    Ok(rate) => rate
                };
                // save user to db
                let trusted = trust(rating);
                match conn.add_user(&user, trusted) {
                    Err(e) => return Err(e), // no signal sent
                    Ok(_) => {
                        tkn.ease();
                        tx.send(Message::NoteUserProgress(user.clone())).unwrap();
                        // memorize
                        user_map.insert(user, trusted);
                    }
                }
            },
            // seen already, memorize
            Ok(Some(v)) => { user_map.insert(user, v); },
            // Error, no signal sent
            Err(e) => return Err(e)
        };
    }
    // we have info on every user
    Ok(Some(user_map))
}

/// Err => Unrecoverable error, no signal sent
/// None => bgg is busy, must ask again later
/// true => last page has been reached
/// false => need to dig deeper
fn check_game(tx: &Sender<Message>, conn: &db::DbConn, client: &Client,
        tkn: &mut RegulationToken, game: &mut Game) -> Result<Option<bool>, Error> {
    // ask for user ratings
    tx.send(Message::NoteGameProgress(game.clone())).unwrap();
    let user_page = bgg::get_users_from(&client, game.id, game.page);
    let users = match user_page {
        Err(e) => {
            tkn.harden(); // wait a bit longer before next request
            tx.send(Message::NoteErr(e)).unwrap();
            // get to the next loop iter
            return Ok(None); // need to reiterate
        },
        Ok(vec) => {
            tkn.ease();
            vec
        }
    };
    if users.is_empty() {
        game.page += 1;
        return Ok(Some(true)); // no users, the last page has been reached
    }

    let mut avg = Avg::new(game.votes, game.rating);
    // check user trust
    let user_map = check_users(tx, conn, client, tkn, &users)?;
    let user_map = match user_map {
        None => return Ok(None), // need to reiterate, http failed
        Some(m) => m
    };
    for (user, rating) in users.iter() {
        if user_map.get(user) == Some(&true) {
            avg.add(*rating);
        }
    }
    // update game stats
    game.rating = avg.result();
    game.votes = avg.n();
    game.page += 1;
    Ok(Some(false))
}

fn runner(config: Config, running: Arc<AtomicBool>, tx: Sender<Message>, mut game: Game) -> () {
    // Configure thread
    let conn = match db::DbConn::new() {
            Err(e) => {
                tx.send(Message::DieErr(e)).unwrap();
                return;
            },
            Ok(cn) => cn
    };
    let client = Client::new();
    let delay_step = Duration::from_millis(config.delay as u64);
    let mut tkn = RegulationToken::new(config.attempts, delay_step);
    loop {
        // check if token stop flag is raised
        if tkn.is_stopped() {
            let e = failure::err_msg("Regulation token stopped the process.");
            tx.send(Message::DieErr(e)).unwrap();
            return;
        }
        // check if we got stop command
        if !running.load(Ordering::SeqCst) {
            tx.send(Message::DieInterrupt).unwrap();
            return;
        }

        // Wait a bit
        thread::sleep(tkn.delay());
        // Start doing main job
        match check_game(&tx, &conn, &client, &mut tkn, &mut game) {
            Err(e) => {
                // propagate error
                tx.send(Message::DieErr(e)).unwrap();
                return;
            },
            Ok(None) => continue, // recoverable err occured, skip to the next iteration
            Ok(Some(false)) => {
                // update game data
                match conn.update_game(&game, false) {
                    Err(e) => {
                        tx.send(Message::DieErr(e)).unwrap();
                        return;
                    },
                    Ok(()) => continue // gathered some data, skip to the next iteration
                };
            },
            Ok(Some(true)) => {// gathered all data
                // update game data
                match conn.update_game(&game, true) {
                    Err(e) => {
                        tx.send(Message::DieErr(e)).unwrap();
                        return;
                    },
                    Ok(()) => tx.send(Message::DieResult(game)).unwrap()
                };
                return
            }
        };
    }
}

pub fn stabilize(config: Config, running: Arc<AtomicBool>, mut progress: impl FnMut(Message) -> ()) -> Result<(), Error> {
     // NB. Errors from mpsc channels use unwrap(). If channels fail,
     // the core of the programm is severely damaged, panic is the only option. 
    
    // Channel for communication
    let (tx, rx) = mpsc::channel();
    let pool = ThreadPool::new(config.threads);

    let games = db::get_unstable_games()?; 
    let job_size = games.len();
    for game in games {
        let tx = tx.clone();
        let running = running.clone();
        pool.execute(move || runner(config, running, tx, game) );
    }

    // This will block main until iterator yields None
    // which will never happen in case of threadpool
    let mut result = Ok(());
    let mut finished = 0;
    for received in rx {
        // handle messages
        match received {
            Message::DieErr(e) => {
                // stop every thread
                running.store(false, Ordering::SeqCst);
                result = Err(e);
                finished += 1;
            },
            Message::DieResult(game) => {
                finished += 1;
                progress(Message::DieResult(game));
            },
            Message::DieInterrupt => finished += 1, 
            msg => progress(msg)
        }
        if finished == job_size { break; } // every thread died somehow
    }
    pool.join();
    result
}

pub fn config() -> Result<Config, Error> {
    let conf = fs::read_to_string(CONFIG_FILE_NAME)
        .with_context(|_| format!("Can't open: {}", CONFIG_FILE_NAME))?;
    let conf = from_str(&conf)?;
    Ok(conf)
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy)]
pub struct Config {
    pub limit: u32, // number or user ratings for a game
    pub attempts: u32, // number or errors that thread can handle before stop
    pub delay: u32, // ms, delay increase after every failure
    pub threads: usize // number of threads
}

impl Config {
    fn new(limit: u32, attempts: u32, delay: u32, threads: usize) -> Config {
        Config {limit, attempts, delay, threads}
    }
}

#[derive(Debug)]
pub enum Message {
    DieErr(Error), // thread must stop after that message
    DieResult(Game), // thread must stop after that message
    DieInterrupt, // thread must stop after that message
    NoteErr(Error),
    NoteUserProgress(User),
    NoteGameProgress(Game)
}

struct RegulationToken {
    limit: u32,
    delay_step: Duration,
    i: u32,
}

impl RegulationToken {
    fn new(limit: u32, delay_step: Duration) -> RegulationToken {
        RegulationToken { limit, delay_step, i: 0 }
    }
    fn delay(&self) -> Duration {
        self.delay_step * self.i
    }
    fn is_stopped(&self) -> bool {
        self.i >= self.limit
    }
    fn ease(&mut self) -> () {
        if !self.is_stopped() && self.i != 0 {
            self.i -= 1;
        }
    }
    fn harden(&mut self) -> () {
        self.i += 1;
    }
}

struct Avg {
    n: u32,
    val: f64
}

impl Avg {
    fn new(n: u32, val: f64) -> Avg {
        Avg {n, val}
    }
    fn add(&mut self, nmbr: f64) -> () {
        self.n += 1;
        self.val = (nmbr + (self.n - 1) as f64 * self.val) / self.n as f64;
    }
    fn result(&self) -> f64 {
        self.val
    }
    fn n(&self) -> u32 {
        self.n
    }
}
