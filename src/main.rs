use std::collections::{HashSet, VecDeque};

use scraper::{Html, Selector};
use url::Url;

type AppId = u32;

fn page_for_app(id: AppId) -> String {
    format!("https://store.steampowered.com/app/{id}/")
}

#[derive(Debug)]
struct App {
    id: AppId,
    name: String,
    tags: Vec<String>,
    price: String,
}

impl App {
    fn new(id: AppId, name: String, tags: Vec<String>, price: String) -> Self {
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
    ids: VecDeque<AppId>,
    apps: Vec<App>,
}

impl Crawler {
    fn new() -> Self {
        Default::default()
    }

    fn crawl(&mut self, initial: AppId) -> color_eyre::Result<()> {
        self.ids.push_back(initial);
        while let Some(id) = self.ids.pop_front() {
            self.crawl_id(id)?;
        }
        Ok(())
    }

    fn crawl_id(&mut self, id: AppId) -> color_eyre::Result<()> {
        let page = ureq::get(&page_for_app(id)).call()?.into_string()?;
        let document = Html::parse_document(&page);
        let tag_selector = Selector::parse(".app_tag").unwrap();
        let tags: Vec<_> = document
            .select(&tag_selector)
            .map(|e| e.inner_html().trim().to_string())
            .collect();
        let price_selector = Selector::parse(".price").unwrap();
        let price = document
            .select(&price_selector)
            .next()
            .unwrap()
            .inner_html()
            .trim()
            .to_string();
        let name_selector = Selector::parse(".apphub_AppName").unwrap();
        let name = document
            .select(&name_selector)
            .next()
            .unwrap()
            .inner_html()
            .trim()
            .to_string();

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

        dbg!(&links);

        let app = App::new(id, name, tags, price);
        dbg!(app);
        self.ids
            .append(&mut links.into_iter().collect::<VecDeque<_>>());
        Ok(())
    }
}

fn main() -> color_eyre::Result<()> {
    let mut crawler = Crawler::new();
    crawler.crawl(400)?; // Portal
    Ok(())
}
