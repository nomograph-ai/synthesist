// Throwaway script to verify storr v3 migration round-trips through GraphView.
// Run via: cargo run --release --example verify_storr_v3 (after copying to examples/)

use nomograph_claim::{
    graph_view::{rebuild, select, GraphView},
    log::LogReader,
};

fn main() -> anyhow::Result<()> {
    let claims_dir = std::path::Path::new("/tmp/storr-v3");

    let reader = LogReader::new(claims_dir)?;
    let mut total = 0usize;
    for item in reader.iter_claims() {
        item?;
        total += 1;
    }
    println!("LogReader: {} claims", total);

    let view = GraphView::open_in_memory()?;
    let stats = rebuild(&view, claims_dir)?;
    println!(
        "Rebuild: {} claims loaded, {} triples, {} ms, {} parse failures",
        stats.claims_loaded, stats.triples_count, stats.duration_ms, stats.parse_failures
    );

    let q = r#"
        PREFIX rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#>
        SELECT ?type (COUNT(?c) AS ?n)
        WHERE { GRAPH ?g { ?c rdf:type ?type } }
        GROUP BY ?type
        ORDER BY DESC(?n)
    "#;
    let results = select(&view, q)?;
    println!("By type:");
    for row in &results.rows {
        println!("  {} {}", row[0].as_str(), row[1].as_str());
    }

    Ok(())
}
