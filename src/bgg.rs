use failure::{Error, ResultExt, bail};
use reqwest::Client;
use reqwest::StatusCode;
use select::document::Document;
use select::predicate::{Name, Class};
use crate::lib::{Game, User};

pub const USER_PAGE_SIZE: u32 = 100;

pub fn get_users_from(client: &Client, game_id: u32, page: u32) -> Result<Vec<(User, f64)>, Error> {
    let url =  format!(
        "https://www.boardgamegeek.com/xmlapi2/thing?type=boardgame&id={}&ratingcomments=1&page={}&pagesize={}",
        game_id,
        page,
        USER_PAGE_SIZE
    );
    let resp = client.get(&url).send()
        .with_context(|_| format!("could not download page `{}`", url))?;
    if resp.status() != StatusCode::OK {
        bail!("Can't get page {} for {}. Status: {}", page, game_id, resp.status());
    }
    let doc = Document::from_read(resp)?;
    filter_users(doc)
}

fn filter_users(doc: Document) -> Result<Vec<(User, f64)>, Error> {
    let usertags = doc.find(Name("comment"));

    let mut users = Vec::new();
    for tag in usertags {
        let name = match tag.attr("username") {
            Some(n) => String::from(n),
            _ => bail!("Can't parse username in the user list")
        };
        let rating = match tag.attr("rating") {
            Some(r) => r.parse::<f64>()?,
            _ => bail!("Can't parse user rating in the user list")
        };
        users.push((name, rating));
    }
    Ok(users)
}

pub struct GameIterator<'a> {
    client: &'a Client,
    page: u32,
    user_limit: u32,
    seen: Option<Game>
}

impl<'a> GameIterator<'a> {
    pub fn new(client: &'a Client, user_limit: u32) -> GameIterator {
        GameIterator {client, page: 0 , user_limit, seen: None}
    }
}

impl<'a> Iterator for GameIterator<'a> {
    type Item = Result<Vec<Game>, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.page += 1;
        // get games from a page
        match get_games_from(self.client, self.page, self.user_limit) {
            Ok(games) => {
                if games.first() == self.seen.as_ref() || games.is_empty() {
                    None
                } else {
                    self.seen = Some(games[0].clone());
                    Some(Ok(games))
                }
            },
            Err(e) => Some(Err(e))
        }
    }
}

fn get_games_from(client: &Client, page: u32, user_limit: u32) -> Result<Vec<Game>, Error> {
    let url =  format!(
        "https://boardgamegeek.com/search/boardgame/page/{}?advsearch=1&range%5Bnumvoters%5D%5Bmin%5D={}&nosubtypes%5B0%5D=boardgameexpansion",
        page,
        user_limit
    );
    let resp = client.get(&url).send()
        .with_context(|_| format!("could not download page `{}`", url))?;
    if resp.status() != StatusCode::OK {
        bail!("Can't get games from {}", page);
    }
    let doc = Document::from_read(resp)?;
    filter_games(doc)
}

fn filter_games(doc: Document) -> Result<Vec<Game>, Error> {
    let rows = doc
        .find(Class("collection_table"))
        .flat_map(|c| c.find(Name("tr"))).skip(1); // skip header

    let mut games = Vec::new();
    for row in rows {
        let mut r = row.find(Name("td"));
        let link = r.nth(2);
        let bgg_geek_rating = r.nth(0);
        let bgg_avg_rating = r.nth(0);
        let bgg_num_votes = r.nth(0);

        let link = match link {
            Some(node) => match node.find(Name("a")).nth(0) {
                Some(l) => l,
                None => bail!("Could not find game link.")
            },
            None => bail!("Could not find game link.") 
        };
        let id = match link.attr("href") {
            Some(href) => href_to_id(href)?,
            None => bail!("Could not find game id.")
        };
        let bgg_geek_rating = match bgg_geek_rating{
            Some(node) => node.text().trim().parse::<f64>()?,
            None => bail!("Could not find geek rating.")
        };
        let bgg_avg_rating = match bgg_avg_rating{
            Some(node) => node.text().trim().parse::<f64>()?,
            None => bail!("Could not find avg rating.")
        };
        let bgg_num_votes = match bgg_num_votes{
            Some(node) => node.text().trim().parse::<u32>()?,
            None => bail!("Could not find num votes.")
        };

        games.push(Game {
            id,
            name: link.text(),
            rating: 0.0,
            votes: 0,
            bgg_num_votes,
            bgg_geek_rating,
            bgg_avg_rating,
            page: 1
        });
    }
    Ok(games)
}

fn href_to_id(href: &str) -> Result<u32, Error> {
    let parts: Vec<&str> = href.rsplit('/').take(2).collect();
    let id = match parts.get(1) {
        Some(x) => x.parse::<u32>()?,
        None => bail!("Can't parse id of the game: {}", href)
    };
    Ok(id)
}

pub fn get_user_average_rating(client: &Client, user: &User) -> Result<f64, Error> {
    let url =  format!("https://boardgamegeek.com/user/{}", user);
    let resp = client.get(&url).send()
        .with_context(|_| format!("could not download page `{}`", url))?;
    if resp.status() != StatusCode::OK {
        bail!("Can't get user average for {}", user);
    }
    let doc = Document::from_read(resp)?;
    let rating = doc
        .find(Class("profile_block")).skip(3).take(1)
        .flat_map(|pb| pb.find(Name("table"))).skip(5).take(1)
        .flat_map(|t| t.find(Name("tr"))).skip(2).take(1)
        .flat_map(|tr| tr.find(Name("td"))).nth(1);
    let rating = match rating {
        None => bail!("Can't find rating element"),
        Some(r) => r.text().parse::<f64>()?
    };
    Ok(rating)
}
