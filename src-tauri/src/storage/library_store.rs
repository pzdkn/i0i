use std::fs;
use std::path::PathBuf;

use rusqlite::{params, Connection};
use tauri::{AppHandle, Manager};

use crate::domain::library::{
    LibrarySnapshot, Paper, PaperDraft, Vault, VaultDraft, VaultPaper, VaultRenameDraft,
};

type StoreResult<T> = Result<T, String>;

pub struct LibraryStore {
    db_path: PathBuf,
}

#[derive(Clone)]
struct SeedVault {
    id: &'static str,
    title: &'static str,
    path: &'static str,
}

#[derive(Clone)]
struct SeedPaper {
    id: &'static str,
    title: &'static str,
    authors: &'static [&'static str],
    venue: &'static str,
    year: i32,
    citations: i32,
    tags: &'static [&'static str],
    note_count: i32,
    annotation_count: i32,
    status: &'static str,
    abstract_text: Option<&'static str>,
}

impl LibraryStore {
    pub fn new(app: &AppHandle) -> StoreResult<Self> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|error| error.to_string())?;
        fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;

        Ok(Self {
            db_path: app_data_dir.join("library.sqlite"),
        })
    }

    pub fn init(&self) -> StoreResult<()> {
        let mut conn = self.open_connection()?;
        self.create_schema(&conn)?;

        if self.is_library_empty(&conn)? {
            self.seed_defaults(&mut conn)?;
        }

        Ok(())
    }

    pub fn get_library(&self) -> StoreResult<LibrarySnapshot> {
        let conn = self.open_connection()?;
        self.read_library(&conn)
    }

    pub fn add_paper_to_vaults(
        &self,
        paper: &PaperDraft,
        vault_ids: &[String],
    ) -> StoreResult<LibrarySnapshot> {
        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let authors_json = to_json(&paper.authors)?;
        let tags_json = to_json(&paper.tags)?;

        tx.execute(
            "
            insert into papers (
              id, title, authors_json, venue, year, citations, tags_json,
              note_count, annotation_count, status, abstract, created_at, updated_at
            )
            values (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, 0, ?8, ?9, datetime('now'), datetime('now'))
            on conflict(id) do update set
              title = excluded.title,
              authors_json = excluded.authors_json,
              venue = excluded.venue,
              year = excluded.year,
              citations = excluded.citations,
              tags_json = excluded.tags_json,
              status = excluded.status,
              abstract = excluded.abstract,
              updated_at = datetime('now')
            ",
            params![
                paper.id,
                paper.title,
                authors_json,
                paper.venue,
                paper.year,
                paper.citations,
                tags_json,
                paper.status,
                paper.abstract_text,
            ],
        )
        .map_err(|error| error.to_string())?;

        for vault_id in vault_ids {
            tx.execute(
                "
                insert into vault_papers (vault_id, paper_id, added_at)
                values (?1, ?2, datetime('now'))
                on conflict(vault_id, paper_id) do nothing
                ",
                params![vault_id, paper.id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn create_vault(&self, draft: &VaultDraft) -> StoreResult<LibrarySnapshot> {
        let normalized = normalize_vault_path(&draft.path)?;
        let title = vault_title_from_path(&normalized)?;
        let id = vault_id_from_path(&normalized)?;
        let conn = self.open_connection()?;

        conn.execute(
            "
            insert into vaults (id, title, path, created_at, updated_at)
            values (?1, ?2, ?3, datetime('now'), datetime('now'))
            ",
            params![id, title, normalized],
        )
        .map_err(|error| {
            let message = error.to_string();
            if message.contains("UNIQUE constraint failed: vaults.path") {
                format!("Vault path already exists: {normalized}")
            } else if message.contains("UNIQUE constraint failed: vaults.id") {
                format!("Vault id already exists: {id}")
            } else {
                message
            }
        })?;

        self.get_library()
    }

    pub fn rename_vault(&self, draft: &VaultRenameDraft) -> StoreResult<LibrarySnapshot> {
        let normalized = normalize_vault_path(&draft.path)?;
        let title = vault_title_from_path(&normalized)?;
        let conn = self.open_connection()?;
        let updated = conn
            .execute(
                "
                update vaults
                set title = ?1, path = ?2, updated_at = datetime('now')
                where id = ?3
                ",
                params![title, normalized, draft.id],
            )
            .map_err(|error| {
                let message = error.to_string();
                if message.contains("UNIQUE constraint failed: vaults.path") {
                    format!("Vault path already exists: {normalized}")
                } else {
                    message
                }
            })?;

        if updated == 0 {
            return Err(format!("Vault not found: {}", draft.id));
        }

        self.get_library()
    }

    pub fn delete_vault(&self, vault_id: &str) -> StoreResult<LibrarySnapshot> {
        if vault_id.trim().is_empty() {
            return Err("Vault id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute("delete from vaults where id = ?1", params![vault_id])
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!("Vault not found: {vault_id}"));
        }

        // Deleting a Vault cascades its membership rows. Papers are shared
        // entities, so remove only the ones that lost their final membership.
        tx.execute(
            "
            delete from papers
            where not exists (
              select 1 from vault_papers
              where vault_papers.paper_id = papers.id
            )
            ",
            [],
        )
        .map_err(|error| error.to_string())?;

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn remove_paper_from_vault(
        &self,
        vault_id: &str,
        paper_id: &str,
    ) -> StoreResult<LibrarySnapshot> {
        if vault_id.trim().is_empty() {
            return Err("Vault id cannot be empty".to_string());
        }

        if paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        let mut conn = self.open_connection()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let deleted = tx
            .execute(
                "delete from vault_papers where vault_id = ?1 and paper_id = ?2",
                params![vault_id, paper_id],
            )
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!(
                "Paper {paper_id} is not linked to Vault {vault_id}"
            ));
        }

        let remaining_memberships: i64 = tx
            .query_row(
                "select count(*) from vault_papers where paper_id = ?1",
                params![paper_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;

        if remaining_memberships == 0 {
            tx.execute("delete from papers where id = ?1", params![paper_id])
                .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())?;
        self.get_library()
    }

    pub fn delete_paper_globally(&self, paper_id: &str) -> StoreResult<LibrarySnapshot> {
        if paper_id.trim().is_empty() {
            return Err("Paper id cannot be empty".to_string());
        }

        let conn = self.open_connection()?;
        let deleted = conn
            .execute("delete from papers where id = ?1", params![paper_id])
            .map_err(|error| error.to_string())?;

        if deleted == 0 {
            return Err(format!("Paper not found: {paper_id}"));
        }

        self.get_library()
    }

    fn open_connection(&self) -> StoreResult<Connection> {
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        conn.execute_batch("pragma foreign_keys = on;")
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

    fn create_schema(&self, conn: &Connection) -> StoreResult<()> {
        conn.execute_batch(
            "
            create table if not exists vaults (
              id text primary key,
              title text not null,
              path text not null unique,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists papers (
              id text primary key,
              title text not null,
              authors_json text not null,
              venue text not null,
              year integer not null,
              citations integer not null default 0,
              tags_json text not null,
              note_count integer not null default 0,
              annotation_count integer not null default 0,
              status text not null,
              abstract text,
              created_at text not null,
              updated_at text not null
            );

            create table if not exists vault_papers (
              vault_id text not null,
              paper_id text not null,
              added_at text not null,
              primary key (vault_id, paper_id),
              foreign key (vault_id) references vaults(id) on delete cascade,
              foreign key (paper_id) references papers(id) on delete cascade
            );
            ",
        )
        .map_err(|error| error.to_string())
    }

    fn is_library_empty(&self, conn: &Connection) -> StoreResult<bool> {
        let count: i64 = conn
            .query_row("select count(*) from vaults", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;

        Ok(count == 0)
    }

    fn seed_defaults(&self, conn: &mut Connection) -> StoreResult<()> {
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        for vault in default_vaults() {
            tx.execute(
                "
                insert into vaults (id, title, path, created_at, updated_at)
                values (?1, ?2, ?3, datetime('now'), datetime('now'))
                ",
                params![vault.id, vault.title, vault.path],
            )
            .map_err(|error| error.to_string())?;
        }

        for paper in default_papers() {
            tx.execute(
                "
                insert into papers (
                  id, title, authors_json, venue, year, citations, tags_json,
                  note_count, annotation_count, status, abstract, created_at, updated_at
                )
                values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, datetime('now'), datetime('now'))
                ",
                params![
                    paper.id,
                    paper.title,
                    to_json_slice(paper.authors)?,
                    paper.venue,
                    paper.year,
                    paper.citations,
                    to_json_slice(paper.tags)?,
                    paper.note_count,
                    paper.annotation_count,
                    paper.status,
                    paper.abstract_text,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        for (vault_id, paper_id) in default_memberships() {
            tx.execute(
                "
                insert into vault_papers (vault_id, paper_id, added_at)
                values (?1, ?2, datetime('now'))
                on conflict(vault_id, paper_id) do nothing
                ",
                params![vault_id, paper_id],
            )
            .map_err(|error| error.to_string())?;
        }

        tx.commit().map_err(|error| error.to_string())
    }

    fn read_library(&self, conn: &Connection) -> StoreResult<LibrarySnapshot> {
        Ok(LibrarySnapshot {
            vaults: read_vaults(conn)?,
            papers: read_papers(conn)?,
            vault_papers: read_vault_papers(conn)?,
        })
    }
}

fn read_vaults(conn: &Connection) -> StoreResult<Vec<Vault>> {
    let mut stmt = conn
        .prepare("select id, title, path from vaults order by path")
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(Vault {
                id: row.get(0)?,
                title: row.get(1)?,
                path: row.get(2)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_papers(conn: &Connection) -> StoreResult<Vec<Paper>> {
    let mut stmt = conn
        .prepare(
            "
            select id, title, authors_json, venue, year, citations, tags_json,
                   note_count, annotation_count, status, abstract
            from papers
            order by updated_at desc, title
            ",
        )
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            let authors_json: String = row.get(2)?;
            let tags_json: String = row.get(6)?;

            Ok(Paper {
                id: row.get(0)?,
                title: row.get(1)?,
                authors: from_json(&authors_json),
                venue: row.get(3)?,
                year: row.get(4)?,
                citations: row.get(5)?,
                tags: from_json(&tags_json),
                note_count: row.get(7)?,
                annotation_count: row.get(8)?,
                status: row.get(9)?,
                abstract_text: row.get(10)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn read_vault_papers(conn: &Connection) -> StoreResult<Vec<VaultPaper>> {
    let mut stmt = conn
        .prepare("select vault_id, paper_id from vault_papers order by added_at desc")
        .map_err(|error| error.to_string())?;

    let rows = stmt
        .query_map([], |row| {
            Ok(VaultPaper {
                vault_id: row.get(0)?,
                paper_id: row.get(1)?,
            })
        })
        .map_err(|error| error.to_string())?;

    collect_rows(rows)
}

fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> StoreResult<Vec<T>> {
    let mut values = Vec::new();
    for row in rows {
        values.push(row.map_err(|error| error.to_string())?);
    }
    Ok(values)
}

fn to_json(values: &[String]) -> StoreResult<String> {
    serde_json::to_string(values).map_err(|error| error.to_string())
}

fn to_json_slice(values: &[&str]) -> StoreResult<String> {
    serde_json::to_string(values).map_err(|error| error.to_string())
}

fn from_json(value: &str) -> Vec<String> {
    serde_json::from_str(value).unwrap_or_default()
}

fn normalize_vault_path(input: &str) -> StoreResult<String> {
    let mut parts = Vec::new();

    for part in input.trim().split('/') {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }

    if parts.is_empty() {
        return Err("Vault path cannot be empty".to_string());
    }

    Ok(format!("/{}", parts.join("/")))
}

fn vault_title_from_path(path: &str) -> StoreResult<String> {
    path.rsplit('/')
        .find(|part| !part.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| "Vault path must include a name".to_string())
}

fn vault_id_from_path(path: &str) -> StoreResult<String> {
    let slug = path
        .trim_matches('/')
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if slug.is_empty() {
        return Err("Vault path must include a usable name".to_string());
    }

    Ok(slug)
}

fn default_vaults() -> Vec<SeedVault> {
    vec![
        SeedVault {
            id: "attention",
            title: "attention",
            path: "/transformers/attention",
        },
        SeedVault {
            id: "self-supervised",
            title: "self-supervised",
            path: "/self-supervised",
        },
        SeedVault {
            id: "vision-transformers",
            title: "vision-transformers",
            path: "/vision-transformers",
        },
        SeedVault {
            id: "interpretability",
            title: "interpretability",
            path: "/interpretability",
        },
        SeedVault {
            id: "scaling",
            title: "scaling-laws",
            path: "/transformers/scaling-laws",
        },
    ]
}

fn default_papers() -> Vec<SeedPaper> {
    vec![
        SeedPaper {
            id: "vaswani2017",
            title: "Attention Is All You Need",
            authors: &["A. Vaswani", "N. Shazeer", "N. Parmar", "J. Uszkoreit", "L. Jones", "A. Gomez"],
            venue: "NeurIPS",
            year: 2017,
            citations: 134821,
            tags: &["foundational", "transformer", "attention"],
            note_count: 4,
            annotation_count: 27,
            status: "READ",
            abstract_text: Some("The Transformer replaces recurrence with attention, making sequence modeling more parallelizable and establishing the architecture behind modern language and vision models."),
        },
        SeedPaper {
            id: "caron2021",
            title: "Emerging Properties in Self-Supervised Vision Transformers",
            authors: &["M. Caron", "H. Touvron", "I. Misra", "H. Jegou", "J. Mairal"],
            venue: "ICCV",
            year: 2021,
            citations: 8842,
            tags: &["frontier", "ssl", "vit", "dino"],
            note_count: 2,
            annotation_count: 14,
            status: "READING",
            abstract_text: Some("Self-supervised ViT features show segmentation-like structure and strong nearest-neighbor classification behavior without labels."),
        },
        SeedPaper {
            id: "dosovitskiy2020",
            title: "An Image is Worth 16x16 Words: Transformers for Image Recognition at Scale",
            authors: &["A. Dosovitskiy", "L. Beyer", "A. Kolesnikov"],
            venue: "ICLR",
            year: 2021,
            citations: 41203,
            tags: &["vit", "foundational"],
            note_count: 3,
            annotation_count: 11,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "devlin2018",
            title: "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding",
            authors: &["J. Devlin", "M. Chang", "K. Lee", "K. Toutanova"],
            venue: "NAACL",
            year: 2019,
            citations: 102045,
            tags: &["foundational", "transformer"],
            note_count: 1,
            annotation_count: 6,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "radford2019",
            title: "Language Models are Unsupervised Multitask Learners",
            authors: &["A. Radford", "J. Wu", "R. Child", "D. Luan"],
            venue: "OpenAI",
            year: 2019,
            citations: 14820,
            tags: &["gpt", "foundational"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "he2022",
            title: "Masked Autoencoders Are Scalable Vision Learners",
            authors: &["K. He", "X. Chen", "S. Xie"],
            venue: "CVPR",
            year: 2022,
            citations: 7041,
            tags: &["ssl", "vit", "mae"],
            note_count: 2,
            annotation_count: 8,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "chen2020",
            title: "A Simple Framework for Contrastive Learning of Visual Representations",
            authors: &["T. Chen", "S. Kornblith", "M. Norouzi", "G. Hinton"],
            venue: "ICML",
            year: 2020,
            citations: 18472,
            tags: &["ssl", "contrastive", "simclr"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "grill2020",
            title: "Bootstrap Your Own Latent: A New Approach to Self-Supervised Learning",
            authors: &["J. Grill", "F. Strub", "F. Altche"],
            venue: "NeurIPS",
            year: 2020,
            citations: 6293,
            tags: &["ssl", "byol"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
        SeedPaper {
            id: "oquab2023",
            title: "DINOv2: Learning Robust Visual Features without Supervision",
            authors: &["M. Oquab", "T. Darcet", "T. Moutakanni"],
            venue: "TMLR",
            year: 2024,
            citations: 1832,
            tags: &["ssl", "dino", "frontier"],
            note_count: 1,
            annotation_count: 0,
            status: "READING",
            abstract_text: None,
        },
        SeedPaper {
            id: "assran2023",
            title: "Self-Supervised Learning from Images with a Joint-Embedding Predictive Architecture",
            authors: &["M. Assran", "Q. Duval", "I. Misra"],
            venue: "CVPR",
            year: 2023,
            citations: 612,
            tags: &["ssl", "jepa", "frontier"],
            note_count: 0,
            annotation_count: 0,
            status: "UNREAD",
            abstract_text: None,
        },
        SeedPaper {
            id: "tay2022",
            title: "Efficient Transformers: A Survey",
            authors: &["Y. Tay", "M. Dehghani", "D. Bahri", "D. Metzler"],
            venue: "ACM CSUR",
            year: 2022,
            citations: 1480,
            tags: &["survey", "efficient"],
            note_count: 0,
            annotation_count: 0,
            status: "UNREAD",
            abstract_text: None,
        },
        SeedPaper {
            id: "kaplan2020",
            title: "Scaling Laws for Neural Language Models",
            authors: &["J. Kaplan", "S. McCandlish", "T. Henighan"],
            venue: "arXiv",
            year: 2020,
            citations: 5821,
            tags: &["scaling", "foundational"],
            note_count: 0,
            annotation_count: 0,
            status: "READ",
            abstract_text: None,
        },
    ]
}

fn default_memberships() -> Vec<(&'static str, &'static str)> {
    vec![
        ("attention", "vaswani2017"),
        ("attention", "caron2021"),
        ("attention", "dosovitskiy2020"),
        ("attention", "devlin2018"),
        ("attention", "radford2019"),
        ("attention", "he2022"),
        ("attention", "chen2020"),
        ("attention", "grill2020"),
        ("attention", "oquab2023"),
        ("attention", "assran2023"),
        ("attention", "tay2022"),
        ("attention", "kaplan2020"),
        ("self-supervised", "caron2021"),
        ("self-supervised", "he2022"),
        ("self-supervised", "chen2020"),
        ("self-supervised", "grill2020"),
        ("self-supervised", "oquab2023"),
        ("self-supervised", "assran2023"),
        ("self-supervised", "dosovitskiy2020"),
        ("vision-transformers", "dosovitskiy2020"),
        ("vision-transformers", "caron2021"),
        ("vision-transformers", "he2022"),
        ("vision-transformers", "oquab2023"),
        ("vision-transformers", "assran2023"),
        ("interpretability", "vaswani2017"),
        ("interpretability", "devlin2018"),
        ("interpretability", "radford2019"),
        ("interpretability", "kaplan2020"),
        ("interpretability", "tay2022"),
        ("scaling", "kaplan2020"),
        ("scaling", "radford2019"),
        ("scaling", "devlin2018"),
        ("scaling", "tay2022"),
        ("scaling", "vaswani2017"),
    ]
}
