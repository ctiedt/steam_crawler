use std::{
    collections::{HashSet, VecDeque},
    hash::Hash,
    sync::{Arc, RwLock},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use clap::{Parser, ValueEnum};
use scraper::{Html, Selector};
use serde::Serializer;
use serde_derive::Serialize;
use tracing::{info, warn};
use url::Url;

type AppId = u32;

fn page_for_app(id: AppId) -> String {
    format!("https://store.steampowered.com/app/{id}/")
}

fn parse_price(price: &str) -> f32 {
    let price = price.to_lowercase();
    if price.starts_with("free") || price.contains("play with firefly") || price.contains("demo") {
        0.0
    } else {
        let new_price = price
            .replace(',', ".")
            .replace('-', "")
            .chars()
            .take_while(|c| *c != '€')
            .collect::<String>();
        info!(new_price);
        new_price.parse().unwrap_or(0.0)
    }
}

#[derive(Parser)]
#[command(
    about = "Steam Game Info Crawler - specify number of games or time to run and a list of seed IDs (400 is Portal)"
)]
struct Options {
    /// The number of games to crawl
    #[arg(short, long, required_unless_present("time"))]
    count: Option<usize>,
    /// The time to crawl for
    #[arg(short, long, required_unless_present("count"))]
    time: Option<u64>,
    /// The output format (csv or json)
    #[arg(short, long)]
    format: Option<OutputFormat>,
    /// The space-separated list of seed IDs
    #[arg(required = true)]
    seed: Vec<AppId>,
}

#[derive(Clone, ValueEnum)]
enum OutputFormat {
    Json,
    Csv,
}

enum TimeOrCount {
    Time(Duration),
    Count(usize),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
struct App {
    id: AppId,
    name: String,
    #[serde(serialize_with = "flatten_tags")]
    tags: Vec<String>,
    price: f32,
}

impl Hash for App {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.name.hash(state);
        self.tags.hash(state);
    }
}

impl Eq for App {}

fn flatten_tags<S>(tags: &[String], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let tags = tags.join(",");
    serializer.serialize_str(&tags)
}

impl App {
    fn new(id: AppId, name: String, tags: Vec<String>, price: f32) -> Self {
        Self {
            id,
            name,
            tags,
            price,
        }
    }
}

#[derive(Default)]
struct Crawler {
    ids: Arc<RwLock<VecDeque<AppId>>>,
    should_not_crawl: Arc<RwLock<Vec<AppId>>>,
    apps: Arc<RwLock<HashSet<App>>>,
    threads: VecDeque<JoinHandle<color_eyre::Result<()>>>,
}

impl Crawler {
    fn new() -> Self {
        Default::default()
    }

    fn apps(&self) -> Vec<App> {
        self.apps.read().unwrap().clone().into_iter().collect()
    }

    fn crawl(&mut self, initial: &[AppId], time_or_count: TimeOrCount) -> color_eyre::Result<()> {
        for id in initial {
            self.ids.write().unwrap().push_back(*id);
        }
        let started_at = Instant::now();

        loop {
            let id = self.ids.write().unwrap().pop_front();
            if let Some(id) = id {
                match time_or_count {
                    TimeOrCount::Time(time) => {
                        let app_known = self.apps.read().unwrap().iter().any(|app| app.id == id);
                        let should_not_crawl = self.should_not_crawl.read().unwrap().contains(&id);
                        if started_at.elapsed() < time && !app_known && !should_not_crawl {
                            let ids = self.ids.clone();
                            let should_not_crawl = self.should_not_crawl.clone();
                            let apps = self.apps.clone();
                            self.threads.push_back(std::thread::spawn(move || {
                                crawl_id(id, ids, should_not_crawl, apps)
                            }));
                        }
                    }
                    TimeOrCount::Count(count) => {
                        let len = self.apps.read().unwrap().len();
                        let app_known = self.apps.read().unwrap().iter().any(|app| app.id == id);
                        let should_not_crawl = self.should_not_crawl.read().unwrap().contains(&id);
                        if len < count && !app_known && !should_not_crawl {
                            let ids = self.ids.clone();
                            let should_not_crawl = self.should_not_crawl.clone();
                            let apps = self.apps.clone();
                            self.threads.push_back(std::thread::spawn(move || {
                                crawl_id(id, ids, should_not_crawl, apps)
                            }));
                        }
                    }
                }
            } else {
                let len = self.apps.read().unwrap().len();
                if let TimeOrCount::Count(count) = time_or_count {
                    if len < count {
                        info!("{len} entries");
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    } else {
                        break;
                    }
                }
            }
        }

        while let Some(thread) = self.threads.pop_front() {
            thread.join().unwrap()?;
        }

        Ok(())
    }
}

fn crawl_id(
    id: AppId,
    ids: Arc<RwLock<VecDeque<AppId>>>,
    should_not_crawl: Arc<RwLock<Vec<AppId>>>,
    apps: Arc<RwLock<HashSet<App>>>,
) -> color_eyre::Result<()> {
    info!("Crawling id {id}");
    let page = ureq::get(&page_for_app(id))
        .set(
            "Cookie",
            "wants_mature_content=1; birthtime=1101855601; lastagecheckage=1-0-2000",
        )
        .call()?
        .into_string()?;
    let document = Html::parse_document(&page);

    let link_selector = Selector::parse("a").unwrap();
    let links: HashSet<AppId> = document
        .select(&link_selector)
        .filter_map(|e| {
            e.value()
                .attr("href")
                .filter(|&href| href.starts_with("https://store.steampowered.com/app/"))
        })
        .map(|link| {
            let url = Url::parse(link).unwrap();
            url.path_segments()
                .unwrap()
                .nth(1)
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect();

    ids.write()
        .unwrap()
        .append(&mut links.into_iter().collect::<VecDeque<_>>());

    let tag_selector = Selector::parse(".app_tag").unwrap();
    let tags: Vec<_> = document
        .select(&tag_selector)
        .map(|e| e.inner_html().trim().to_string())
        .filter(|tag| tag != "+")
        .collect();
    let price_selector = Selector::parse(".price").unwrap();
    let purchase_selector = Selector::parse(".game_purchase_action").unwrap();
    let price = document
        .select(&purchase_selector)
        .map(|action| {
            if let Some(id) = action.value().id() {
                if id == "dlc_purchase_action" {
                    return 0.0;
                }
            }

            match action.select(&price_selector).next() {
                Some(price_element) => parse_price(price_element.inner_html().trim()),
                None => 0.0,
            }
        })
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    if price.is_none() {
        info!("Skipping invalid app {id}");
        should_not_crawl.write().unwrap().push(id);
        return Ok(());
    }

    let price = price.unwrap();
    let name_selector = Selector::parse(".apphub_AppName").unwrap();
    let name = document
        .select(&name_selector)
        .next()
        .unwrap()
        .inner_html()
        .trim()
        .to_string();

    let app = App::new(id, name, tags, price);
    apps.write().unwrap().insert(app);
    Ok(())
}

fn main() -> color_eyre::Result<()> {
    let opts = Options::parse();

    let time_or_count = if let Some(t) = opts.time {
        TimeOrCount::Time(Duration::from_secs(t))
    } else {
        TimeOrCount::Count(opts.count.unwrap())
    };

    tracing::subscriber::set_global_default(
        tracing_subscriber::FmtSubscriber::builder()
            .with_writer(std::io::stderr)
            .finish(),
    )?;

    let mut crawler = Crawler::new();

    if let Err(e) = crawler.crawl(&opts.seed, time_or_count) {
        warn!("An error occured during crawling: {e:?}. Printing possibly invalid data.")
    }

    match opts.format {
        Some(OutputFormat::Json) => {
            let apps = serde_json::to_string(&crawler.apps())?;
            println!("{apps}")
        }
        _ => {
            let mut apps = csv::WriterBuilder::default()
                .delimiter(b';')
                .from_writer(std::io::stdout());
            for app in crawler.apps() {
                apps.serialize(app)?;
            }
        }
    }

    Ok(())
}
