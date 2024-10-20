use bevy::app::{App, Plugin};
use std::future::Future;

use bevy::asset::{AssetLoader, AsyncReadExt, LoadContext};

use bevy::reflect::TypePath;

use arrow_array::{Float32Array, Int32Array, RecordBatch, StringArray};
use arrow_schema::SchemaRef;
use parquet::arrow::arrow_reader::{ParquetRecordBatchReader, ParquetRecordBatchReaderBuilder};

use arrow_array::cast::downcast_array;
use bevy::asset::io::Reader;
use bevy::prelude::*;
use bevy::utils::{ConditionalSendFuture, Instant};
use std::marker::PhantomData;

use bytes::Bytes;
use thiserror::Error;

#[derive(serde::Deserialize, Asset, TypePath, Debug)]
pub struct Point {
    pub node_uuid: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(serde::Deserialize, Asset, TypePath, Debug)]
pub struct PointCloudData {
    pub points: Vec<Point>,
}

pub struct ParquetAssetPlugin {
    extensions: Vec<&'static str>,
    _marker: PhantomData<PointCloudData>,
}

impl Plugin for ParquetAssetPlugin {
    fn build(&self, app: &mut App) {
        app.init_asset::<PointCloudData>()
            .register_asset_loader(ParquetAssetLoader {
                extensions: self.extensions.clone(),
                _marker: PhantomData::<PointCloudData>,
            });
    }
}

impl ParquetAssetPlugin {
    /// Create a new plugin that will load assets from files with the given extensions.
    pub fn new(extensions: &[&'static str]) -> Self {
        Self {
            extensions: extensions.to_owned(),
            _marker: PhantomData::<PointCloudData>,
        }
    }
}

struct ParquetAssetLoader {
    extensions: Vec<&'static str>,
    _marker: PhantomData<PointCloudData>,
}

#[derive(Debug, Error)]
pub enum ParquetLoaderError {
    /// An [IO Error](std::io::Error)
    #[error("Could not read the file: {0}")]
    Io(#[from] std::io::Error),
}

impl AssetLoader for ParquetAssetLoader {
    type Asset = PointCloudData;
    type Settings = ();
    type Error = ParquetLoaderError;

    fn load<'a>(
        &'a self,
        reader: &'a mut Reader,
        _settings: &'a Self::Settings,
        _load_context: &'a mut LoadContext,
    ) -> impl ConditionalSendFuture
           + Future<Output = Result<<Self as AssetLoader>::Asset, <Self as AssetLoader>::Error>>
    {
        Box::pin(async move {
            let start = Instant::now();

            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;

            let rdr: ParquetRecordBatchReader =
                ParquetRecordBatchReaderBuilder::try_new(Bytes::from(bytes.to_vec()))
                    .unwrap()
                    .with_batch_size(10000)
                    .build()
                    .unwrap();
            let batches: Vec<RecordBatch> = rdr.collect::<Result<Vec<_>, _>>().unwrap();
            let schema = batches[0].schema();

            fn batch_to_points(b: &RecordBatch, schema: &SchemaRef) -> Vec<Point> {
                let node_list: StringArray =
                    downcast_array(b.column(schema.index_of("node_uuid").unwrap()));
                let x_list: Float32Array =
                    downcast_array(b.column(schema.index_of("point_x").unwrap()));
                let y_list: Float32Array =
                    downcast_array(b.column(schema.index_of("point_y").unwrap()));
                let z_list: Float32Array =
                    downcast_array(b.column(schema.index_of("point_z").unwrap()));
                let r_list: Int32Array = downcast_array(b.column(schema.index_of("r").unwrap()));
                let g_list: Int32Array = downcast_array(b.column(schema.index_of("g").unwrap()));
                let b_list: Int32Array = downcast_array(b.column(schema.index_of("b").unwrap()));

                let count: usize = x_list.len();
                let mut points: Vec<Point> = Vec::with_capacity(count);
                let mut i: usize = 0;
                while i < count {
                    let node_uuid = String::from(node_list.value(i));
                    let x: f32 = x_list.value(i);
                    let y: f32 = y_list.value(i);
                    let z: f32 = z_list.value(i);
                    let r: u8 = u8::try_from(r_list.value(i)).unwrap();
                    let g: u8 = u8::try_from(g_list.value(i)).unwrap();
                    let b: u8 = u8::try_from(b_list.value(i)).unwrap();
                    if x != 0.0f32 && y != 0.0f32 && z != 0.0f32 {
                        points.push(Point {
                            node_uuid: node_uuid,
                            x: x,
                            y: y,
                            z: z,
                            r: r,
                            g: g,
                            b: b,
                        });
                    }
                    i += 1
                }
                return points;
            }
            let points: Vec<Point> = batches
                .iter()
                .map(|b| batch_to_points(b, &schema))
                .collect::<Vec<_>>()
                .into_iter()
                .flatten()
                .collect();
            let duration = start.elapsed();
            info!(
                "Loaded {} points in {} ms",
                points.len(),
                duration.as_millis()
            );
            Ok(PointCloudData { points })
        })
    }
    // fn load<'a>(
    //     &'a self,
    //     reader: &'a mut Reader,
    //     load_context: &'a mut LoadContext,
    // ) -> BoxedFuture<'a, Result<(), anyhow::Error>> {

    // }

    fn extensions(&self) -> &[&str] {
        &self.extensions
    }
}
