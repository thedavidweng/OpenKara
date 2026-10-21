use super::*;
use crate::source_host::test_support::{fixture_manifest, FixtureDistribution, FixturePackage};
use crate::source_host::SourceHost;
use serde_json::json;
use std::sync::Arc;

fn workflow_package(id: &str, web: bool) -> FixturePackage {
    let mut manifest = fixture_manifest(id, "1.0.0");
    manifest.name = if web {
        "Official fixture"
    } else {
        "Challenge fixture"
    }
    .into();
    manifest.authentication = if web {
        json!({"flow":"web","sign_in_url":"https://official.openkara.test/login","navigation_origins":["https://official.openkara.test"],"cookie_names":["official_cookie"]})
    } else {
        json!({"flow":"plugin","presentations":["qr","text"]})
    };
    manifest.capabilities = ["playlists", "playlist_tracks", "track_details", "import"]
        .map(str::to_owned)
        .to_vec();
    if web {
        manifest
            .capabilities
            .extend(["search", "liked_tracks", "preview"].map(str::to_owned));
    }
    manifest.requires_authentication = [
        "playlists",
        "playlist.tracks",
        "tracks.get",
        "media.resolve",
    ]
    .map(str::to_owned)
    .to_vec();
    if web {
        manifest
            .requires_authentication
            .retain(|method| method != "media.resolve");
    }
    if !web {
        manifest
            .capabilities
            .retain(|capability| capability != "track_details");
    }
    manifest.media_origins = vec!["https://raw.githubusercontent.com".into()];
    manifest.resource_origins = vec!["https://raw.githubusercontent.com".into()];
    FixturePackage {
        manifest,
        worker: include_str!("../../../fixtures/streaming-source/workflow-worker.mjs").into(),
    }
}
const MEDIA_ROOT: &str = "https://raw.githubusercontent.com/thedavidweng/OpenKara/f439d2d94edff292b2f38b10bbe279a2e355e7dc/src-tauri/tests/fixtures/audio/fixture";

#[tokio::test]
#[ignore = "requires explicitly prepared exact Node runtime in OPENKARA_SOURCE_TEST_NODE"]
async fn installed_sources_keep_auth_pages_metadata_and_shared_import_isolated() {
    let directory = tempfile::tempdir().expect("temporary data");
    std::env::set_var(
        "OPENKARA_TEST_CREDENTIAL_STORE_DIR",
        directory.path().join("credentials"),
    );
    let mut distribution = FixtureDistribution::new().await;
    distribution
        .publish(vec![
            workflow_package("official", true),
            workflow_package("challenge", false),
        ])
        .await;
    let mut config = distribution.config(directory.path().join("host"));
    config.add_fixture_resource(
        format!("{MEDIA_ROOT}.wav"),
        vec!["93.184.216.34".parse().unwrap()],
        include_bytes!("../../tests/fixtures/audio/fixture.wav").to_vec(),
    );
    let mut second_audio = std::io::Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(
            &mut second_audio,
            hound::WavSpec {
                channels: 1,
                sample_rate: 8000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for index in 0..8000i16 {
            writer.write_sample(index.wrapping_mul(17)).unwrap();
        }
        writer.finalize().unwrap();
    }
    config.add_fixture_resource(
        format!("{MEDIA_ROOT}.wav?source=challenge"),
        vec!["93.184.216.34".parse().unwrap()],
        second_audio.into_inner(),
    );
    let host = SourceHost::new(config).await.expect("open real host");
    assert_eq!(host.refresh().await.expect("verified discovery").len(), 2);
    for id in ["official", "challenge"] {
        host.install(id, None)
            .await
            .expect("actual signed install and handshake");
        host.set_enabled(id, true, false).await.expect("enable");
    }
    let cache = Arc::new(TrackMetadataCache::default());
    let root = directory.path().to_owned();
    let worker_host = host.clone();
    let metadata = cache.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let official=PluginStreamingSource::open(worker_host.clone(),"official".into(),root.clone()).unwrap().with_cache(metadata.clone());
        let challenge=PluginStreamingSource::open(worker_host.clone(),"challenge".into(),root.clone()).unwrap().with_cache(metadata.clone());
        assert!(!official.session().unwrap().signed_in);
        assert!(!challenge.session().unwrap().signed_in);
        let public=official.track_page("search",json!({"query":"public","cursor":null})).unwrap();
        assert_eq!(public.tracks[0].remote_track_id,"1");
        assert!(public.tracks[0].cover_url.is_none());
        assert!(official.tracks(&["1".into()]).is_err());
        assert_eq!(official.import_tracks(&["1".into()]).unwrap()[0].title,"Official fixture 1");
        let public_media=official.resolve("1",&NoResolveProgress).unwrap();
        match public_media{super::types::StreamingResolveOutcome::File(file)=>std::fs::remove_file(file.path).unwrap(),_=>panic!("accepted public import media")};
        let start=official.request("auth.start",json!({})).unwrap();
        assert_eq!(start["presentation"]["kind"],"web");
        let failed=official.request("auth.complete",json!({"challenge":start["challenge"],"credentials":{"official_cookie":"wrong"}}));
        assert!(failed.is_err());
        assert!(load_credentials(&root,"official",&worker_host.description("official").unwrap().owner_binding).unwrap().is_none());
        let verified=official.request("auth.complete",json!({"challenge":start["challenge"],"credentials":{"official_cookie":"verified-cookie"}})).unwrap();
        assert_eq!(official.save_verified_session(&verified).unwrap().display_name.as_deref(),Some("Official fixture singer"));
        let start=challenge.request("auth.start",json!({})).unwrap();
        assert_eq!(start["presentation"]["kind"],"qr");
        assert_eq!(challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap()["state"],"pending");
        let complete=challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap();
        assert_eq!(challenge.save_verified_session(&complete).unwrap().display_name.as_deref(),Some("Challenge fixture singer"));
        let first=official.playlist_tracks_page("42",None).unwrap();
        let other=challenge.playlist_tracks_page("42",None).unwrap();
        assert_eq!(first.tracks[0].remote_track_id,other.tracks[0].remote_track_id);
        assert_ne!(first.tracks[0].title,other.tracks[0].title);
        assert!(official.session().unwrap().signed_in);
        assert!(challenge.playlist_tracks_page("42",first.next_cursor.as_deref()).is_err());
        assert_eq!(official.playlist_tracks_page("42",first.next_cursor.as_deref()).unwrap().tracks[0].remote_track_id,"3");
        assert!(challenge.search("public").is_err());
        assert!(official.tracks(&["missing".into()]).is_err());
        assert_eq!(metadata.get("official","1").unwrap().artist,"Official fixture artist");
        assert_eq!(metadata.get("challenge","1").unwrap().artist,"Challenge fixture artist");
        let preview=match official.preview("1").unwrap(){StreamingPreviewOutcome::Stream(resolved)=>resolved,_=>panic!("accepted preview")};
        let preview_lease=worker_host.acquire_work("official",crate::source_host::WorkKind::Preview).unwrap();
        tauri::async_runtime::block_on(async {
            let mut response=worker_host.open_media("official",&preview.descriptor,Some((0,Some(43))),preview_lease.cancellation()).await.unwrap();
            assert_eq!(response.status,206);
            assert_eq!(response.chunk().await.unwrap().unwrap(),include_bytes!("../../tests/fixtures/audio/fixture.wav")[0..44]);
            assert!(worker_host.set_enabled("official",false,false).await.is_err());
        });
        drop(preview_lease);
        assert!(challenge.preview("1").is_err());
        let library=crate::library_root::LibraryRoot::create(&root.join("library")).unwrap();
        let mut connection=rusqlite::Connection::open_in_memory().unwrap();
        crate::cache::apply_migrations(&connection).unwrap();
        for source in [&official,&challenge] {
            let tracks=source.import_tracks(&["1".into()]).unwrap();
            let progress=run_streaming_import(&mut connection,&library,source,StreamingImportRequest{source_id:source.source_id().into(),items:items_from_tracks(&tracks),playlist:None}, |_|LibraryDecisionAction::Cancel).unwrap();
            assert_eq!(progress.imported_song_ids.len(),1);
            assert_eq!(progress.source_id,source.source_id());
        }
        let first=lookup_song_hash(&connection,&StreamingTrackIdentity{source:"official".into(),remote_track_id:"1".into()}).unwrap().unwrap();
        let second=lookup_song_hash(&connection,&StreamingTrackIdentity{source:"challenge".into(),remote_track_id:"1".into()}).unwrap().unwrap();
        assert_ne!(first,second);
        let lease=worker_host.acquire_work("official",crate::source_host::WorkKind::Import).unwrap();
        assert!(tauri::async_runtime::block_on(worker_host.set_enabled("official",false,false)).is_err());
        assert!(worker_host.snapshot("official").unwrap().enabled);
        drop(lease);
        tauri::async_runtime::block_on(worker_host.set_enabled("official",false,false)).unwrap();
        assert!(load_credentials(&root,"official",&worker_host.description("official").unwrap().owner_binding).unwrap().is_some());
        tauri::async_runtime::block_on(worker_host.uninstall("official",false)).unwrap();
        assert_eq!(lookup_song_hash(&connection,&StreamingTrackIdentity{source:"official".into(),remote_track_id:"1".into()}).unwrap().as_deref(),Some(first.as_str()));
        assert_eq!(lookup_song_hash(&connection,&StreamingTrackIdentity{source:"challenge".into(),remote_track_id:"1".into()}).unwrap().as_deref(),Some(second.as_str()));
        assert!(load_credentials(&root,"official","other-owner/package").unwrap().is_none());
        challenge.sign_out().unwrap();
        assert!(load_credentials(&root,"official",&worker_host.description("official").unwrap().owner_binding).unwrap().is_some());
        assert!(!challenge.session().unwrap().signed_in);
    }).await.unwrap();
    assert!(host.snapshot("challenge").unwrap().enabled);
    assert!(cache.get("challenge", "1").is_some());
    std::env::remove_var("OPENKARA_TEST_CREDENTIAL_STORE_DIR");
}
