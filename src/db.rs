use crate::lib::{Game, User};
use chrono::Local;
use failure::{bail, Error};
use rusqlite::types::ToSql;
use rusqlite::{Connection, OpenFlags, NO_PARAMS};

const DB_FILE_NAME: &str = "top.db";

pub fn initialize() -> Result<(), Error> {
    let conn = Connection::open(DB_FILE_NAME)?;
    // create db file
    conn.execute(
        "create table if not exists games (
            id integer primary key,
            name text not null,
            rating real,
            num_votes integer,
            updated datetime,
            stable integer,
            bgg_num_votes integer,
            bgg_geek_rating real,
            bgg_avg_rating real,
            page integer
         )",
        NO_PARAMS,
    )?;
    conn.execute(
        "create table if not exists users (
            name text primary key,
            updated datetime,
            trusted integer
         )",
        NO_PARAMS,
    )?;
    Ok(())
}

pub fn drop_all_games() -> Result<(), Error> {
    let conn = Connection::open(DB_FILE_NAME)?;
    conn.execute("delete from games", NO_PARAMS)?;
    Ok(())
}

pub fn add_games(games: Vec<Game>) -> Result<(), Error> {
    let mut conn = Connection::open(DB_FILE_NAME)?;
    let tx = conn.transaction()?;
    let now = Local::now();
    for game in games {
        tx.execute("insert into games (id, name, updated, stable, bgg_num_votes, bgg_geek_rating, bgg_avg_rating, page, num_votes, rating) 
        values (?1, ?2, ?3, 0, ?4, ?5, ?6, 1, 0, 0)",
            &[&game.id as &ToSql, &game.name, &now.to_string(), &game.bgg_num_votes, &game.bgg_geek_rating, &game.bgg_avg_rating])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn get_unstable_games() -> Result<Vec<Game>, Error> {
    let conn = Connection::open(DB_FILE_NAME)?;
    let mut stmt = conn.prepare(
        "select id, name, page, num_votes, rating from games where not stable order by random()",
    )?;
    let iter = stmt.query_map(NO_PARAMS, |r| Game {
        id: r.get(0),
        name: r.get(1),
        page: r.get(2),
        votes: r.get(3),
        rating: r.get(4),
        bgg_avg_rating: 0.0,
        bgg_geek_rating: 0.0,
        bgg_num_votes: 0,
    })?;
    let mut gameboxes = Vec::new();
    for gamebox in iter {
        gameboxes.push(gamebox?);
    }
    Ok(gameboxes)
}

pub struct DbConn {
    conn: Connection,
}

impl DbConn {
    pub fn new() -> Result<DbConn, Error> {
        let conn = Connection::open_with_flags(
            DB_FILE_NAME,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX, // for multi thread
        )?;
        Ok(DbConn { conn })
    }

    pub fn add_user(&self, user: &User, trusted: bool) -> Result<(), Error> {
        let now = Local::now();
        match self.conn.execute(
            "insert or ignore into users (name, updated, trusted) values (?1, ?2, ?3)",
            &[&user as &ToSql, &now.to_string(), &trusted],
        ) {
            Ok(_) => Ok(()),
            Err(err) => bail!(err),
        }
    }

    pub fn get_number_of_unstable_games(&self) -> Result<u32, Error> {
        let mut stmt = self
            .conn
            .prepare("select count(*) from games where not stable")?;
        let count: u32 = stmt.query_row(NO_PARAMS, |r| r.get(0))?;
        Ok(count)
    }

    pub fn check_user(&self, user: &User) -> Result<Option<bool>, Error> {
        let mut stmt = self
            .conn
            .prepare("select trusted from users where name = ?")?;
        let result: Option<bool> = match stmt.query_row(&[user as &ToSql], |r| -> bool { r.get(0) })
        {
            Ok(true) => Some(true),                            // trusted
            Ok(false) => Some(false),                          // not trusted
            Err(rusqlite::Error::QueryReturnedNoRows) => None, // not seen
            Err(e) => bail!(e),
        };
        Ok(result)
    }

    pub fn get_all_games(&self) -> Result<Vec<Game>, Error> {
        let conn = Connection::open(DB_FILE_NAME)?;
        let mut stmt = conn.prepare("SELECT id, name, rating, num_votes, bgg_num_votes, bgg_geek_rating, bgg_avg_rating FROM games order by rating desc")?;
        let games_iter = stmt.query_map(NO_PARAMS, |row| Game {
            id: row.get(0),
            name: row.get(1),
            rating: row.get(2),
            votes: row.get(3),
            bgg_num_votes: row.get(4),
            bgg_geek_rating: row.get(5),
            bgg_avg_rating: row.get(6),
            page: 0,
        })?;
        let mut games = Vec::new();
        for game in games_iter {
            games.push(game?);
        }
        Ok(games)
    }

    pub fn update_game(&self, game: &Game, stable: bool) -> Result<(), Error> {
        let now = Local::now();
        match self.conn.execute("UPDATE games SET page = ?1, stable = ?2, rating = ?3, num_votes = ?4, updated = ?5 WHERE id = ?6",
                &[&game.page as &ToSql, &stable, &game.rating, &game.votes, &now.to_string(), &game.id]) {
            Ok(_) => Ok(()),
            Err(err) => bail!(err)
        }
    }
}
