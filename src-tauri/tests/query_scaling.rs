use openkara_lib::{
    cache::{
        self,
        query_trace::{detect_n_plus_one, trace_queries},
    },
    library::{
        playlist::{add_songs_to_playlist, create_playlist, get_playlist_songs, list_playlists},
        songs::{set_songs_instrumental, set_songs_language},
        Song,
    },
};
use rusqlite::Connection;

fn make_song(index: usize) -> Song {
    Song {
        hash: format!("song-{index:04}"),
        file_path: Some(format!("media/song-{index:04}.mp3")),
        cdg_path: None,
        media_g_container: None,
        instrumental: false,
        language: None,
        audio_source_kind: "original".to_owned(),
        title: Some(format!("Starlight Anthem {index}")),
        artist: Some("Aurora Ensemble".to_owned()),
        album: Some("Midnight Stage".to_owned()),
        duration_ms: 180_000,
        has_cover_art: false,
        artwork_thumb_path: None,
        cover_art: None,
        imported_at: 1_700_000_000 + index as i64,
        original_ext: Some("mp3".to_owned()),
    }
}

fn seeded_connection(song_count: usize) -> (Connection, Vec<String>, String) {
    let mut conn = Connection::open_in_memory().expect("in-memory sqlite should open");
    cache::apply_migrations(&conn).expect("migrations should apply");

    let mut song_ids = Vec::with_capacity(song_count);
    for index in 0..song_count {
        let song = make_song(index);
        song_ids.push(song.hash.clone());
        cache::upsert_song(&conn, &song).expect("song insert should succeed");
    }

    let primary_playlist = create_playlist(&conn, "Friday Karaoke".to_owned())
        .expect("primary playlist should create")
        .id;
    add_songs_to_playlist(&mut conn, &primary_playlist, &song_ids)
        .expect("playlist songs should insert");

    let playlist_total = (song_count / 2).max(2);
    for p in 1..playlist_total {
        let extra = create_playlist(&conn, format!("Setlist {p}"))
            .expect("extra playlist should create")
            .id;
        let subset = &song_ids[..song_ids.len().min(3)];
        add_songs_to_playlist(&mut conn, &extra, subset).expect("subset songs should insert");
    }

    (conn, song_ids, primary_playlist)
}

#[test]
fn library_and_playlist_queries_execute_constant_sql_without_n_plus_one_growth() {
    let (mut small_conn, small_ids, small_playlist_id) = seeded_connection(5);
    let (mut large_conn, large_ids, large_playlist_id) = seeded_connection(50);

    let (small_list, small_list_trace) =
        trace_queries(&mut small_conn, |conn| cache::list_songs(conn).unwrap()).unwrap();
    let (large_list, large_list_trace) =
        trace_queries(&mut large_conn, |conn| cache::list_songs(conn).unwrap()).unwrap();
    assert_eq!(small_list.len(), 5);
    assert_eq!(large_list.len(), 50);
    assert_eq!(
        detect_n_plus_one(
            "cache::list_songs",
            5,
            &small_list_trace,
            50,
            &large_list_trace
        ),
        None
    );

    let (small_search, small_search_trace) = trace_queries(&mut small_conn, |conn| {
        cache::search_songs(conn, "Starlight*").unwrap()
    })
    .unwrap();
    let (large_search, large_search_trace) = trace_queries(&mut large_conn, |conn| {
        cache::search_songs(conn, "Starlight*").unwrap()
    })
    .unwrap();
    assert_eq!(small_search.len(), 5);
    assert_eq!(large_search.len(), 50);
    assert_eq!(
        detect_n_plus_one(
            "cache::search_songs",
            5,
            &small_search_trace,
            50,
            &large_search_trace
        ),
        None
    );

    let (_, small_playlists_trace) =
        trace_queries(&mut small_conn, |conn| list_playlists(conn).unwrap()).unwrap();
    let (_, large_playlists_trace) =
        trace_queries(&mut large_conn, |conn| list_playlists(conn).unwrap()).unwrap();
    assert_eq!(
        detect_n_plus_one(
            "playlist::list_playlists",
            2,
            &small_playlists_trace,
            25,
            &large_playlists_trace
        ),
        None
    );

    let (_, small_ps_trace) = trace_queries(&mut small_conn, |conn| {
        get_playlist_songs(conn, &small_playlist_id).unwrap()
    })
    .unwrap();
    let (_, large_ps_trace) = trace_queries(&mut large_conn, |conn| {
        get_playlist_songs(conn, &large_playlist_id).unwrap()
    })
    .unwrap();
    assert_eq!(
        detect_n_plus_one(
            "playlist::get_playlist_songs",
            5,
            &small_ps_trace,
            50,
            &large_ps_trace
        ),
        None
    );

    let (updated_inst, large_inst_trace) = trace_queries(&mut large_conn, |conn| {
        set_songs_instrumental(conn, &large_ids, true).unwrap()
    })
    .unwrap();
    let (_, small_inst_trace) = trace_queries(&mut small_conn, |conn| {
        set_songs_instrumental(conn, &small_ids, true).unwrap()
    })
    .unwrap();
    assert_eq!(updated_inst.len(), 50);
    assert!(updated_inst.iter().all(|song| song.instrumental));
    assert_eq!(large_inst_trace.statement_count(), 2);
    assert_eq!(
        detect_n_plus_one(
            "songs::set_songs_instrumental",
            5,
            &small_inst_trace,
            50,
            &large_inst_trace
        ),
        None
    );

    let (updated_lang, large_lang_trace) = trace_queries(&mut large_conn, |conn| {
        set_songs_language(conn, &large_ids, Some("ja")).unwrap()
    })
    .unwrap();
    let (_, small_lang_trace) = trace_queries(&mut small_conn, |conn| {
        set_songs_language(conn, &small_ids, Some("ja")).unwrap()
    })
    .unwrap();
    assert_eq!(updated_lang.len(), 50);
    assert!(updated_lang
        .iter()
        .all(|song| song.language.as_deref() == Some("ja")));
    assert_eq!(large_lang_trace.statement_count(), 2);
    assert_eq!(
        detect_n_plus_one(
            "songs::set_songs_language",
            5,
            &small_lang_trace,
            50,
            &large_lang_trace
        ),
        None
    );
}

#[test]
fn batch_song_update_rolls_back_all_rows_when_one_hash_is_missing() {
    let (conn, mut ids, _) = seeded_connection(3);
    ids.push("missing-song-hash".to_owned());

    let err = set_songs_instrumental(&conn, &ids, true)
        .expect_err("missing song should fail the batch update");
    assert!(err.message.contains("missing-song-hash"));

    let songs = cache::list_songs(&conn).expect("songs should load");
    assert!(songs.iter().all(|song| !song.instrumental));
}

#[test]
fn detect_n_plus_one_flags_per_row_lookup_loops() {
    let (mut small_conn, small_ids, _) = seeded_connection(4);
    let (mut large_conn, large_ids, _) = seeded_connection(16);

    let (_, small_trace) = trace_queries(&mut small_conn, |conn| {
        for id in &small_ids {
            let _ = cache::get_song_by_hash(conn, id).unwrap();
        }
    })
    .unwrap();
    let (_, large_trace) = trace_queries(&mut large_conn, |conn| {
        for id in &large_ids {
            let _ = cache::get_song_by_hash(conn, id).unwrap();
        }
    })
    .unwrap();

    let diagnostic = detect_n_plus_one("per_row_lookup", 4, &small_trace, 16, &large_trace)
        .expect("per-row lookup loop must be flagged as N+1");
    assert!(diagnostic.contains("N=4 executed 4"));
    assert!(diagnostic.contains("N=16 executed 16"));
}
