use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow_array::builder::{FixedSizeListBuilder, Float32Builder, Int32Builder, StringBuilder};
use arrow_array::{Array, Float32Array, Int32Array, RecordBatch, RecordBatchIterator, StringArray};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{Connection, Table};

use crate::chunk::Chunk;

const TABLE: &str = "chunks";

fn schema(dim: i32) -> SchemaRef {
    Arc::new(Schema::new(vec![
        Field::new(
            "vector",
            DataType::FixedSizeList(Arc::new(Field::new("item", DataType::Float32, true)), dim),
            true,
        ),
        Field::new("relative", DataType::Utf8, false),
        Field::new("language", DataType::Utf8, false),
        Field::new("line_start", DataType::Int32, false),
        Field::new("line_end", DataType::Int32, false),
        Field::new("hash", DataType::Utf8, false),
        Field::new("text", DataType::Utf8, false),
    ]))
}

pub async fn open_db(out: &Path) -> Result<Connection> {
    Ok(lancedb::connect(out.to_str().context("non-utf8 index path")?)
        .execute()
        .await?)
}

pub async fn table_exists(db: &Connection) -> Result<bool> {
    Ok(db.table_names().execute().await?.iter().any(|t| t == TABLE))
}

/// Open the chunks table, creating it (with the given vector dim) if absent.
pub async fn ensure_table(db: &Connection, dim: i32) -> Result<Table> {
    if table_exists(db).await? {
        Ok(db.open_table(TABLE).execute().await?)
    } else {
        Ok(db.create_empty_table(TABLE, schema(dim)).execute().await?)
    }
}

/// Remove all chunks belonging to the given files (used before re-adding a
/// changed file, and to drop removed files).
pub async fn delete_files(table: &Table, relatives: &[String]) -> Result<()> {
    for r in relatives {
        let esc = r.replace('\'', "''");
        table.delete(&format!("relative = '{esc}'")).await?;
    }
    Ok(())
}

pub async fn add_chunks(table: &Table, dim: i32, records: &[(Chunk, Vec<f32>)]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let mut vb = FixedSizeListBuilder::new(Float32Builder::new(), dim);
    let mut rel = StringBuilder::new();
    let mut lang = StringBuilder::new();
    let mut ls = Int32Builder::new();
    let mut le = Int32Builder::new();
    let mut hsh = StringBuilder::new();
    let mut txt = StringBuilder::new();
    for (c, v) in records {
        vb.values().append_slice(v);
        vb.append(true);
        rel.append_value(&c.relative);
        lang.append_value(&c.language);
        ls.append_value(c.line_start as i32);
        le.append_value(c.line_end as i32);
        hsh.append_value(&c.hash);
        txt.append_value(&c.text);
    }
    let batch = RecordBatch::try_new(
        schema(dim),
        vec![
            Arc::new(vb.finish()),
            Arc::new(rel.finish()),
            Arc::new(lang.finish()),
            Arc::new(ls.finish()),
            Arc::new(le.finish()),
            Arc::new(hsh.finish()),
            Arc::new(txt.finish()),
        ],
    )?;
    let batches = RecordBatchIterator::new(vec![Ok(batch)], schema(dim));
    table.add(Box::new(batches)).execute().await?;
    Ok(())
}

pub struct Hit {
    pub relative: String,
    pub language: String,
    pub line_start: i32,
    pub line_end: i32,
    pub distance: f32,
    pub text: String,
}

pub async fn search(index: &Path, query: Vec<f32>, k: usize) -> Result<Vec<Hit>> {
    let db = open_db(index).await?;
    let tbl = db.open_table(TABLE).execute().await?;
    let batches: Vec<RecordBatch> = tbl
        .query()
        .nearest_to(query)?
        .limit(k)
        .execute()
        .await?
        .try_collect()
        .await?;

    let mut hits = Vec::new();
    for b in &batches {
        let rel = col_str(b, "relative")?;
        let lang = col_str(b, "language")?;
        let ls = col_i32(b, "line_start")?;
        let le = col_i32(b, "line_end")?;
        let txt = col_str(b, "text")?;
        let dist = col_f32(b, "_distance")?;
        for i in 0..b.num_rows() {
            hits.push(Hit {
                relative: rel.value(i).to_string(),
                language: lang.value(i).to_string(),
                line_start: ls.value(i),
                line_end: le.value(i),
                distance: dist.value(i),
                text: txt.value(i).to_string(),
            });
        }
    }
    Ok(hits)
}

fn col_str<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a StringArray> {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<StringArray>())
        .with_context(|| format!("column {name} (utf8) missing"))
}
fn col_i32<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a Int32Array> {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<Int32Array>())
        .with_context(|| format!("column {name} (i32) missing"))
}
fn col_f32<'a>(b: &'a RecordBatch, name: &str) -> Result<&'a Float32Array> {
    b.column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
        .with_context(|| format!("column {name} (f32) missing"))
}
