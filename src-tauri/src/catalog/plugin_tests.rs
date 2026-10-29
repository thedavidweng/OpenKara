use super::*;
use crate::source_host::test_support::{fixture_manifest, RegistryFixture};
use crate::source_host::{HostConfig, SourceHost, SourceManifest};
use serde_json::json;
use std::sync::Arc;

const API_ORIGIN: &str = "https://api.streaming-fixture.test";
const MEDIA_ROOT: &str = "https://raw.githubusercontent.com/thedavidweng/OpenKara/f439d2d94edff292b2f38b10bbe279a2e355e7dc/src-tauri/tests/fixtures/audio/fixture";

/// One registry package built from the repo's single-file engine fixture: the
/// web package gets the wider capability set, the plugin package the smaller.
fn workflow_manifest(registry: &RegistryFixture, id: &str, web: bool) -> SourceManifest {
    let mut manifest = fixture_manifest(id, "1.0.0");
    manifest.package_id = registry.package_id(id);
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
    } else {
        manifest
            .capabilities
            .retain(|capability| capability != "track_details");
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
    manifest.resource_origins = vec![API_ORIGIN.into()];
    manifest.media_origins = vec!["https://raw.githubusercontent.com".into()];
    manifest
}

fn media_payload(url: &str) -> String {
    json!({"media":{"url":url,"headers":{},"expires_at":null,"content_type":"audio/wav","file_extension":"wav","size_bytes":null,"duration_ms":1000,"bit_rate":null}}).to_string()
}

#[tokio::test]
async fn installed_sources_keep_auth_pages_metadata_and_shared_import_isolated() {
    let directory = tempfile::tempdir().expect("temporary data");
    let mut registry = RegistryFixture::new("test.registry").await;
    registry
        .publish(&[
            workflow_manifest(&registry, "official", true),
            workflow_manifest(&registry, "challenge", false),
        ])
        .await;
    let mut api = mockito::Server::new_async().await;
    for (id, suffix) in [("official", ""), ("challenge", "?source=challenge")] {
        api.mock("GET", format!("/sources/{id}/media/1").as_str())
            .with_header("content-type", "application/json")
            .with_body(media_payload(&format!("{MEDIA_ROOT}.wav{suffix}")))
            .create_async()
            .await;
    }
    let mut config = HostConfig::fixture(directory.path().join("host"), "0.17.0".into());
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
    config.add_fixture_upstream(API_ORIGIN.into(), api.url());
    let host = SourceHost::new(config).await.expect("open real host");
    host.add_registry(&registry.url())
        .await
        .expect("add the fixture registry");
    for id in ["official", "challenge"] {
        host.install_package(
            registry.registry_id(),
            &registry.package_id(id),
            None,
            false,
            false,
        )
        .await
        .expect("verified registry install and describe");
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
        assert!(load_credentials(&root,&worker_host.description("official").unwrap().owner_binding).unwrap().is_none());
        let verified=official.request("auth.complete",json!({"challenge":start["challenge"],"credentials":{"official_cookie":"verified-cookie"}})).unwrap();
        assert_eq!(official.save_verified_session(&verified).unwrap().display_name.as_deref(),Some("Official fixture singer"));
        let start=challenge.request("auth.start",json!({})).unwrap();
        assert_eq!(start["presentation"]["kind"],"qr");
        assert_eq!(challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap()["state"],"pending");
        let complete=challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap();
        assert_eq!(challenge.save_verified_session(&complete).unwrap().display_name.as_deref(),Some("Challenge fixture singer"));
        let describe=tauri::async_runtime::block_on(worker_host.request("challenge","describe",json!({}))).unwrap();
        assert_eq!(describe["storage_keys"],json!(["account_id"]),"the fixture keeps its sign-in in the host KV store");
        let stale=tauri::async_runtime::block_on(worker_host.request("challenge","session.restore",json!({"credentials":{"opaque_session":"stale"}}))).unwrap_err();
        assert_eq!(stale.code,"session_expired");
        let describe=tauri::async_runtime::block_on(worker_host.request("challenge","describe",json!({}))).unwrap();
        assert_eq!(describe["session"]["state"],"expired");
        let start=challenge.request("auth.start",json!({"presentation":"text"})).unwrap();
        assert_eq!(start["presentation"]["kind"],"text");
        assert_eq!(challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap()["state"],"pending");
        let complete=challenge.request("auth.poll",json!({"challenge":start["challenge"]})).unwrap();
        assert_eq!(complete["state"],"complete");
        assert!(challenge.save_verified_session(&complete).unwrap().signed_in);
        tauri::async_runtime::block_on(worker_host.set_enabled("challenge",false,false)).unwrap();
        tauri::async_runtime::block_on(worker_host.set_enabled("challenge",true,false)).unwrap();
        let describe=tauri::async_runtime::block_on(worker_host.request("challenge","describe",json!({}))).unwrap();
        assert_eq!(describe["session"]["state"],"signed_out","a fresh engine starts signed out");
        assert_eq!(describe["storage_keys"],json!(["account_id"]),"the KV store survives an engine restart");
        assert!(challenge.session().unwrap().signed_in,"the adapter restores the stored session");
        let first=official.playlist_tracks_page("42",None).unwrap();
        let other=challenge.playlist_tracks_page("42",None).unwrap();
        assert_eq!(first.tracks[0].remote_track_id,other.tracks[0].remote_track_id);
        assert_ne!(first.tracks[0].title,other.tracks[0].title);
        assert!(official.session().unwrap().signed_in);
        assert!(challenge.playlist_tracks_page("42",first.next_cursor.as_deref()).is_err());
        let second=official.playlist_tracks_page("42",first.next_cursor.as_deref()).unwrap();
        assert_eq!(second.tracks[0].remote_track_id,"2");
        assert_eq!(second.tracks[1].remote_track_id,"3");
        assert_eq!(second.tracks[0].refusal.as_ref().unwrap().reason,ImportRefusalReason::TrialClip);
        assert!(challenge.search("public").is_err());
        assert!(official.tracks(&["missing".into()]).is_err());
        assert_eq!(metadata.get("official","1").unwrap().artist,"Official fixture artist");
        assert_eq!(metadata.get("challenge","1").unwrap().artist,"Challenge fixture artist");
        match official.resolve("2",&NoResolveProgress).unwrap(){super::types::StreamingResolveOutcome::Refusal(refusal)=>assert_eq!(refusal.reason,ImportRefusalReason::TrialClip),_=>panic!("accepted refused media")};
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
        let official_owner=worker_host.description("official").unwrap().owner_binding;
        assert!(load_credentials(&root,&official_owner).unwrap().is_some());
        tauri::async_runtime::block_on(worker_host.uninstall("official",false)).unwrap();
        assert_eq!(lookup_song_hash(&connection,&StreamingTrackIdentity{source:"official".into(),remote_track_id:"1".into()}).unwrap().as_deref(),Some(first.as_str()));
        assert_eq!(lookup_song_hash(&connection,&StreamingTrackIdentity{source:"challenge".into(),remote_track_id:"1".into()}).unwrap().as_deref(),Some(second.as_str()));
        assert!(load_credentials(&root,"other-owner/package").unwrap().is_none());
        challenge.sign_out().unwrap();
        assert!(load_credentials(&root,&official_owner).unwrap().is_some());
        assert!(!challenge.session().unwrap().signed_in);
        let describe=tauri::async_runtime::block_on(worker_host.request("challenge","describe",json!({}))).unwrap();
        assert_eq!(describe["storage_keys"],json!([]),"sign-out clears the fixture KV entry");
    }).await.unwrap();
    assert!(host.snapshot("challenge").unwrap().enabled);
    assert!(cache.get("challenge", "1").is_some());
}
