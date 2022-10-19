mod parquet_plugin;

use bevy::diagnostic::{Diagnostics, FrameTimeDiagnosticsPlugin};
use bevy::math::DMat4;
use bevy::prelude::*;
use bevy::reflect::TypeUuid;
use bevy::window::PresentMode;
use bevy_common_assets::json::JsonAssetPlugin;
use bevy_inspector_egui::WorldInspectorPlugin;

use crate::parquet_plugin::{ParquetAssetPlugin, PointCloudData};

use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::Sub;
use bevy::render::mesh::PrimitiveTopology;
use bevy::render::render_resource::AsBindGroup;

use smooth_bevy_cameras::LookTransformPlugin;
use smooth_bevy_cameras::controllers::orbit::{OrbitCameraBundle, OrbitCameraController, OrbitCameraPlugin};

#[derive(serde::Deserialize, TypeUuid, Debug)]
#[uuid = "413be529-bfeb-41b3-9db0-4b8b380a2c46"]
struct NodePose {
    #[serde(rename = "nodeUuid")]
    node_uuid: String,
    #[serde(rename = "optPos")]
    opt_pos: [f64; 16],
}

#[derive(serde::Deserialize, TypeUuid, Debug)]
#[uuid = "bbf4de2d-d334-47da-a9a0-501f0488cd6e"]
struct Poses {
    poses: Vec<NodePose>,
}

#[derive(Resource)]
struct PosesHandle(Handle<Poses>);

#[derive(Resource)]
struct ImageHandle(Handle<Image>);

#[derive(Resource)]
struct PointCloudDataHandle(Handle<PointCloudData>);


#[derive(Default, AsBindGroup, TypeUuid, Debug, Clone)]
#[uuid = "ebf24026-f0c7-4e86-8a4a-96a40101d1b5"]
pub struct SimpleMaterial {}

impl Material for SimpleMaterial {
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Opaque
    }
}

#[derive(Component)]
struct TopRightText;

fn main() {
    #[cfg(target_arch = "wasm32")]
    console_error_panic_hook::set_once();

    App::new()
        .insert_resource(ClearColor(Color::WHITE))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            window: WindowDescriptor {
                title: "PCD Reader".to_string(),
                present_mode: PresentMode::AutoNoVsync,
                ..default()
            },
            ..default()
        }).add(bevy::log::LogPlugin {
            // Uncomment this to override the default log settings:
            level: bevy::log::Level::INFO,
            filter: "wgpu=warn,pcd_renderer=trace".to_string(),
            ..default()
        }))
        .add_plugin(MaterialPlugin::<SimpleMaterial>::default())
        .add_plugin(WorldInspectorPlugin::new())
        .add_plugin(JsonAssetPlugin::<Poses>::new(&["json"]))
        .add_plugin(ParquetAssetPlugin::new(&["parquet"]))
        .add_plugin(FrameTimeDiagnosticsPlugin::default())
        .add_plugin(LookTransformPlugin)
        .add_plugin(OrbitCameraPlugin::default())
        .add_startup_system(setup)
        .add_system(update_fps_text_sys)
        .add_system(render_point_cloud)
        .run();
}

fn setup(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    commands.insert_resource(AmbientLight {
        color: Color::WHITE,
        brightness: 1.0,
    });

    let poses: Handle<Poses> = asset_server.load("node_pose.json");
    let node_pose_handle = PosesHandle(poses);
    commands.insert_resource(node_pose_handle);

    let parquet = PointCloudDataHandle(asset_server.load("point_cloud.snappy.parquet"));
    commands.insert_resource(parquet);

    commands
        .spawn(OrbitCameraBundle::new(OrbitCameraController {
            enabled: true,
            mouse_rotate_sensitivity: Vec2::splat(0.5),
            mouse_translate_sensitivity: Vec2::splat(10.0),
            mouse_wheel_zoom_sensitivity: 0.2,
            ..default()
        }, Vec3::new(0.0, 0.0, 30.0), Vec3::ZERO))
        .insert(Camera3dBundle::default());

    let font = asset_server.load("fonts/FiraMono-Medium.ttf");
    commands.spawn(TextBundle {
            style: Style {
                align_self: AlignSelf::FlexEnd,
                position_type: PositionType::Absolute,
                position: UiRect {
                    top: Val::Px(5.0),
                    right: Val::Px(5.0),
                    ..default()
                },
                ..default()
            },
            text: Text {
                sections: vec![
                    TextSection {
                        value: "AAAA".to_string(),
                        style: TextStyle { font: font, font_size: 16.0, color: Color::BLUE },
                    },
                ],
                alignment: Default::default(),
            },
            ..default()
        })
        .insert(TopRightText);
}



fn render_point_cloud(
    mut commands: Commands,
    poses_handle: Res<PosesHandle>,
    pcd_handle: Res<PointCloudDataHandle>,
    mut node_poses: ResMut<Assets<Poses>>,
    mut pcd_data: ResMut<Assets<PointCloudData>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut simple_materials: ResMut<Assets<SimpleMaterial>>,
) {
    if !pcd_data.is_empty() {
        if let Some(poses) = node_poses.remove(poses_handle.0.id()) {
            trace!("render_point_cloud. node_poses len: {}", poses.poses.len());

            let transforms: Vec<DMat4> = poses.poses.iter().map(|v| { DMat4::from_cols_array(&v.opt_pos).transpose() })
                .collect::<Vec<_>>();
            transforms.iter().for_each(|t| { trace!("t: {}", t) });

            let min_transition = transforms.iter().min_by(|a, b| {
                let w_axis_ord = a.w_axis.x.partial_cmp(&b.w_axis.x).unwrap();
                return if w_axis_ord == Ordering::Equal {
                    a.w_axis.y.partial_cmp(&b.w_axis.y).unwrap()
                } else {
                    w_axis_ord
                };
            }
            ).unwrap();
            trace!("min_transition: {}", min_transition);

            let node_to_transform: HashMap<String, Transform> = poses.poses.into_iter().map(|v| {
                // Substract min_transition from nodes translation to make it small number.
                // Otherwise due to f32 impression we lost centimeters precision in numbers like 3620823.7240922246 (UTM coordinate)
                let dmat = DMat4::from_cols_array(&v.opt_pos).transpose();
                let diff_w = dmat.w_axis.sub(min_transition.w_axis);
                trace!("render_point_cloud. diff_w: {}", diff_w);

                let mat = Mat4::from_cols(dmat.x_axis.as_vec4(), dmat.y_axis.as_vec4(), dmat.z_axis.as_vec4(), diff_w.as_vec4());
                let transform = Transform::from_matrix(mat);
                (v.node_uuid, transform)
            })
                .collect::<HashMap<_, _>>();

            node_to_transform.iter().for_each(|(_, transform)| {
                let node_mesh = meshes.add(Mesh::from(shape::Icosphere {
                    radius: 0.3,
                    subdivisions: 5,
                }));
                commands.spawn(PbrBundle {
                    mesh: node_mesh,
                    material: materials.add(StandardMaterial::from(Color::BLUE)),
                    transform: transform.clone(),
                    ..default()
                });
            });

            trace!("render_point_cloud. pcd_data len: {}", pcd_data.len());

            if let Some(pcd_data) = pcd_data.remove(pcd_handle.0.id()) {
                trace!("render_point_cloud. pcd_data len: {}", pcd_data.points.len());

                let mut mesh = Mesh::new(PrimitiveTopology::PointList);
                let mut positions: Vec<[f32; 3]> = Vec::with_capacity(pcd_data.points.len());
                let mut colors: Vec<[f32; 4]> = Vec::with_capacity(pcd_data.points.len());

                for point in &pcd_data.points {
                    let transform = node_to_transform.get(point.node_uuid.as_str()).unwrap();
                    let point_vec3 = Vec3 { x: point.x, y: point.y, z: point.z };
                    // Convert from local point to global
                    let transformed = transform.transform_point(point_vec3);
                    positions.push(transformed.to_array());

                    let color = Color::rgba_u8(point.r, point.g, point.b, 255u8);
                    colors.push(color.as_rgba_f32());
                }
                trace!("render_point_cloud. positions: {}", positions.len());
                trace!("render_point_cloud. colors: {}", colors.len());

                mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);


                commands.spawn(MaterialMeshBundle {
                    mesh: meshes.add(mesh).into(),
                    material: simple_materials.add(SimpleMaterial {}),
                    ..default()
                });
                trace!("render_point_cloud. Created mesh!");
            } else {
                warn!("render_point_cloud. Could not find PCD DATA!");
            }
        }
    }
}

// https://github.com/qhdwight/voxel-game-rs/blob/main/src/main.rs#L320
fn update_fps_text_sys(
    time: Res<Time>,
    diagnostics: Res<Diagnostics>,
    mut query: Query<&mut Text, With<TopRightText>>,
) {
    for mut text in query.iter_mut() {
        let mut fps = 0.0;
        if let Some(fps_diagnostic) = diagnostics.get(FrameTimeDiagnosticsPlugin::FPS) {
            if let Some(fps_avg) = fps_diagnostic.average() {
                fps = fps_avg;
            }
        }

        let mut frame_time = time.delta_seconds_f64();
        if let Some(frame_time_diagnostic) = diagnostics.get(FrameTimeDiagnosticsPlugin::FRAME_TIME) {
            if let Some(frame_time_avg) = frame_time_diagnostic.average() {
                frame_time = frame_time_avg;
            }
        }

        let text = &mut text.sections[0].value;
        text.clear();
        use std::{
            fmt::Write,
        };
        write!(text, "{:.1} fps, {:.3} ms/frame", fps, frame_time * 1000.0).unwrap();
    }
}
