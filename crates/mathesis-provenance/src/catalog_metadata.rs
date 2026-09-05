use crate::model::CatalogMetadata;
use crate::store::{ProvenanceStore, Result};
use rusqlite::{params, OptionalExtension};

pub const CATALOG_SCHEMA_VERSION: u32 = 1;
pub const ENTITY_RESOLUTION_VERSION: &str = "mathesis-taxonomy::resolve-v1";

impl ProvenanceStore {
    pub fn replace_catalog_metadata(&self, metadata: &CatalogMetadata) -> Result<()> {
        self.conn.execute(
            "INSERT INTO catalog_metadata
             (id, schema_version, build_version, entity_resolution_version,
              graph_input_sha256, taxonomy_input_sha256, entity_count, alias_count)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
               schema_version=excluded.schema_version,
               build_version=excluded.build_version,
               entity_resolution_version=excluded.entity_resolution_version,
               graph_input_sha256=excluded.graph_input_sha256,
               taxonomy_input_sha256=excluded.taxonomy_input_sha256,
               entity_count=excluded.entity_count,
               alias_count=excluded.alias_count",
            params![
                metadata.schema_version,
                metadata.build_version,
                metadata.entity_resolution_version,
                metadata.graph_input_sha256,
                metadata.taxonomy_input_sha256,
                metadata.entity_count,
                metadata.alias_count,
            ],
        )?;
        Ok(())
    }

    pub fn catalog_metadata(&self) -> Result<Option<CatalogMetadata>> {
        self.conn
            .query_row(
                "SELECT schema_version, build_version, entity_resolution_version,
                        graph_input_sha256, taxonomy_input_sha256, entity_count, alias_count
                 FROM catalog_metadata WHERE id = 1",
                [],
                |row| {
                    Ok(CatalogMetadata {
                        schema_version: row.get::<_, u32>(0)?,
                        build_version: row.get(1)?,
                        entity_resolution_version: row.get(2)?,
                        graph_input_sha256: row.get(3)?,
                        taxonomy_input_sha256: row.get(4)?,
                        entity_count: row.get(5)?,
                        alias_count: row.get(6)?,
                    })
                },
            )
            .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_metadata_is_replaced_atomically() {
        let store = ProvenanceStore::open_in_memory().unwrap();
        let first = CatalogMetadata {
            schema_version: 1,
            build_version: "catalog-v1".into(),
            entity_resolution_version: "resolve-v1".into(),
            graph_input_sha256: "a".into(),
            taxonomy_input_sha256: "b".into(),
            entity_count: 2,
            alias_count: 3,
        };
        store.replace_catalog_metadata(&first).unwrap();
        assert_eq!(store.catalog_metadata().unwrap(), Some(first));

        let second = CatalogMetadata {
            graph_input_sha256: "c".into(),
            taxonomy_input_sha256: "d".into(),
            entity_count: 4,
            alias_count: 8,
            ..CatalogMetadata {
                schema_version: 1,
                build_version: "catalog-v2".into(),
                entity_resolution_version: "resolve-v2".into(),
                graph_input_sha256: String::new(),
                taxonomy_input_sha256: String::new(),
                entity_count: 0,
                alias_count: 0,
            }
        };
        store.replace_catalog_metadata(&second).unwrap();
        assert_eq!(store.catalog_metadata().unwrap(), Some(second));
    }
}
