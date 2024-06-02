use actix_files as fs;
use actix_multipart::Multipart;
use actix_web::{web, App, Error, HttpResponse, HttpServer, Responder};
use futures_util::{StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::time::SystemTime;
use std::io::Write;
use uuid::Uuid;
use askama::Template;
use serde_json;

const POSTS_PER_PAGE: usize = 30;

#[derive(Serialize, Deserialize, Clone)]
struct Post {
    id: String,
    parent_id: Option<String>,
    title: String,
    message: String,
    file: Option<String>,
    #[serde(default = "default_timestamp")]
    timestamp: u64,
}

impl Post {
    fn file_url(&self) -> Option<&str> {
        self.file.as_deref()
    }

    fn is_image(&self) -> bool {
        if let Some(file_url) = self.file_url() {
            file_url.ends_with(".jpg") || file_url.ends_with(".jpeg") || file_url.ends_with(".png") || file_url.ends_with(".gif") || file_url.ends_with(".webp")
        } else {
            false
        }
    }

    fn is_video(&self) -> bool {
        if let Some(file_url) = self.file_url() {
            file_url.ends_with(".mp4") || file_url.ends_with(".webm")
        } else {
            false
        }
    }

    fn is_audio(&self) -> bool {
        if let Some(file_url) = self.file_url() {
            file_url.ends_with(".mp3")
        } else {
            false
        }
    }
}

fn default_timestamp() -> u64 {
    0
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate<'a> {
    posts: &'a [Post],
    prev_page: Option<usize>,
    next_page: Option<usize>,
}

#[derive(Template)]
#[template(path = "post_view.html")]
struct PostViewTemplate<'a> {
    post: &'a Post,
    replies: &'a [Post],
}

async fn save_post(
    db: web::Data<Db>,
    upload_dir: web::Data<String>,
    mut payload: Multipart,
) -> Result<HttpResponse, Error> {
    let mut title = String::new();
    let mut message = String::new();
    let mut filename: Option<String> = None;
    let mut parent_id: Option<String> = None;

    // Get the current timestamp
    let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs();

    // Process each field in the multipart payload
    while let Ok(Some(mut field)) = payload.try_next().await {
        let content_disposition = field.content_disposition();
        let field_name = content_disposition.get_name().unwrap().to_string();

        match field_name.as_str() {
            "title" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    title.push_str(std::str::from_utf8(&data).unwrap());
                }
            }
            "message" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    message.push_str(std::str::from_utf8(&data).unwrap());
                }
            }
            "parent_id" => {
                while let Some(chunk) = field.next().await {
                    let data = chunk.unwrap();
                    parent_id = Some(std::str::from_utf8(&data).unwrap().to_string());
                }
            }
            "file" => {
                if let Some(filename_value) = content_disposition.get_filename() {
                    if !filename_value.is_empty() {
                        let file_extension = filename_value
                            .split('.')
                            .last()
                            .map(String::from)
                            .unwrap_or_else(|| "tmp".to_string());
                        let file_name = format!("{}.{}", Uuid::new_v4(), file_extension);
                        let filepath = format!("{}/{}", upload_dir.get_ref(), &file_name);

                        let mut f = web::block(|| std::fs::File::create(filepath)).await??;

                        while let Some(chunk) = field.next().await {
                            let data = chunk.unwrap();
                            f = web::block(move || {
                                f.write_all(&data).map(|_| f)
                            }).await??;
                        }

                        filename = Some(file_name);
                    }
                }
            }
            _ => (),
        }
    }

    let post = Post {
        id: Uuid::new_v4().to_string(),
        parent_id,
        title,
        message,
        file: filename.clone(),
        timestamp,
    };

    let serialized = serde_json::to_vec(&post).unwrap();
    db.insert(&post.id, serialized).unwrap();
    
    if let Some(parent_id) = &post.parent_id {
        if let Ok(Some(parent_post_bytes)) = db.get(&parent_id) {
            let mut parent_post: Post = serde_json::from_slice(&parent_post_bytes).unwrap();
            parent_post.timestamp = timestamp;
            let serialized_parent = serde_json::to_vec(&parent_post).unwrap();
            db.insert(&parent_post.id, serialized_parent).unwrap();
        }
    }

    db.flush().unwrap();

    if let Some(parent_id) = post.parent_id {
        Ok(HttpResponse::SeeOther()
            .append_header(("Location", format!("/post/{}", parent_id)))
            .finish())
    } else {
        Ok(HttpResponse::SeeOther()
            .append_header(("Location", "/"))
            .finish())
    }
}

async fn view_post(db: web::Data<Db>, post_id: web::Path<String>) -> impl Responder {
    let mut post = None;
    let mut replies = Vec::new();

    for item in db.iter().values() {
        let current_post: Post = serde_json::from_slice(&item.unwrap()).unwrap_or_else(|_| Post {
            id: String::new(),
            parent_id: None,
            title: String::new(),
            message: String::new(),
            file: None,
            timestamp: 0,
        });

        if current_post.id == *post_id {
            post = Some(current_post.clone());
        } else if let Some(parent_id) = &current_post.parent_id {
            if parent_id == &*post_id {
                replies.push(current_post.clone());
            }
        }
    }

    // Sort replies by timestamp in descending order
    replies.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    replies.reverse();

    if let Some(post) = post {
        let template = PostViewTemplate {
            post: &post,
            replies: &replies,
        };
        HttpResponse::Ok().content_type("text/html").body(template.render().unwrap())
    } else {
        HttpResponse::NotFound().finish()
    }
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<usize>,
}

async fn index(db: web::Data<Db>, query: web::Query<PageQuery>) -> impl Responder {
    let page = query.page.unwrap_or(0);
    let start_index = page * POSTS_PER_PAGE;
    let end_index = start_index + POSTS_PER_PAGE;

    let mut posts = Vec::new();
    for item in db.iter().values() {
        let post: Post = serde_json::from_slice(&item.unwrap()).unwrap_or_else(|_| Post {
            id: String::new(),
            parent_id: None,
            title: String::new(),
            message: String::new(),
            file: None,
            timestamp: 0,
        });
        if post.parent_id.is_none() {
            posts.push(post);
        }
    }

    // Sort posts by timestamp in descending order
    posts.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // Paginate posts
    let paginated_posts: Vec<Post> = posts[start_index..end_index.min(posts.len())].to_vec();

    let prev_page = if page > 0 { Some(page - 1) } else { None };
    let next_page = if end_index < posts.len() { Some(page + 1) } else { None };

    let template = IndexTemplate {
        posts: &paginated_posts,
        prev_page,
        next_page,
    };

    HttpResponse::Ok().content_type("text/html").body(template.render().unwrap())
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db = sled::open("my_db").unwrap();
    let upload_dir = std::env::var("UPLOAD_DIR").unwrap_or_else(|_| "./static/uploads".to_string());
    std::fs::create_dir_all(&upload_dir).unwrap();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(db.clone()))
            .app_data(web::Data::new(upload_dir.clone()))
            .service(fs::Files::new("/static", "./static").show_files_listing())
            .route("/", web::get().to(index))
            .route("/submit", web::post().to(save_post))
            .route("/post/{id}", web::get().to(view_post))
    })
    .bind("0.0.0.0:8080")?
    .run()
    .await
}
