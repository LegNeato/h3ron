use std::collections::HashMap;
use std::hash::Hash;
use std::str::FromStr;

use ndarray::ArrayView2;
use numpy::{Element, PyReadonlyArray2};
use pyo3::{prelude::*, PyNativeType, Python, wrap_pyfunction};

use h3ron::error::check_valid_h3_resolution;
use h3ron_ndarray as h3n;

use crate::{
    array::{
        AxisOrder,
        ResolutionSearchMode,
    },
    collections::H3CompactedVec,
    polygon::Polygon,
    transform::Transform,
};
use crate::error::IntoPyResult;

mod array;
mod transform;
mod collections;
mod polygon;
mod error;

/// version of the module
#[pyfunction]
fn version() -> PyResult<String> { Ok(env!("CARGO_PKG_VERSION").to_string()) }

/// find the h3 resolution closed to the size of a pixel in an array
/// of the given shape with the given transform
#[pyfunction]
pub fn nearest_h3_resolution(shape_any: &PyAny, transform: &Transform, axis_order_str: &str, search_mode_str: &str) -> PyResult<u8> {
    let axis_order = AxisOrder::from_str(axis_order_str)?;
    let search_mode = ResolutionSearchMode::from_str(search_mode_str)?;
    let shape: [usize; 2] = shape_any.extract()?;

    h3n::resolution::nearest_h3_resolution(&shape, &transform.inner, &axis_order.inner, search_mode.inner)
        .into_pyresult()
}

fn array_to_h3<'a, T>(arr: &'a ArrayView2<'a, T>, transform: &'a Transform, nodata_value: &'a Option<T>, h3_resolution: u8, axis_order_str: &str) -> PyResult<HashMap<T, H3CompactedVec>>
    where T: PartialEq + Sized + Sync + Eq + Hash + Element {
    let axis_order = AxisOrder::from_str(axis_order_str)?;
    check_valid_h3_resolution(h3_resolution).into_pyresult()?;

    let conv = h3n::H3Converter::new(&arr, &nodata_value, &transform.inner, axis_order.inner);
    let result = conv.to_h3(h3_resolution, true)
        .into_pyresult()?
        .drain()
        .map(|(value, compacted_vec)| (value.clone(), H3CompactedVec { inner: compacted_vec }))
        .collect();

    Ok(result)
}

macro_rules! make_array_to_h3_variant {
    ($name:ident, $dtype:ty) => {
        #[pyfunction]
        fn $name<'py>(_py: Python<'py>, np_array: PyReadonlyArray2<$dtype>, transform: &Transform, nodata_value: Option<$dtype>, h3_resolution: u8, axis_order_str: &str) -> PyResult<HashMap<$dtype, H3CompactedVec>> {
            let arr = np_array.as_array();
            array_to_h3(&arr, transform, &nodata_value, h3_resolution, axis_order_str)
        }
    }
}
// generate some specialized variants of array_to_h3 to expose to python
make_array_to_h3_variant!(array_to_h3_u8, u8);
make_array_to_h3_variant!(array_to_h3_i8, i8);
make_array_to_h3_variant!(array_to_h3_u16, u16);
make_array_to_h3_variant!(array_to_h3_i16, i16);
make_array_to_h3_variant!(array_to_h3_u32, u32);
make_array_to_h3_variant!(array_to_h3_i32, i32);
make_array_to_h3_variant!(array_to_h3_u64, u64);
make_array_to_h3_variant!(array_to_h3_i64, i64);
//make_array_to_h3_variant!(array_to_h3_f32, f32);
//make_array_to_h3_variant!(array_to_h3_f64, f64);


/// h3ron python bindings
#[pymodule]
fn h3ronpy(_py: Python<'_>, m: &PyModule) -> PyResult<()> {
    env_logger::init(); // run with the environment variable RUST_LOG set to "debug" for log output

    m.add("Transform", m.py().get_type::<Transform>())?;
    m.add("H3CompactedVec", m.py().get_type::<H3CompactedVec>())?;
    m.add("Polygon", m.py().get_type::<Polygon>())?;

    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(nearest_h3_resolution, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_u8, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_i8, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_u16, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_i16, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_u32, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_i32, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_u64, m)?)?;
    m.add_function(wrap_pyfunction!(array_to_h3_i64, m)?)?;
    //m.add_function(wrap_pyfunction!(array_to_h3_f32, m)?)?;
    //m.add_function(wrap_pyfunction!(array_to_h3_f64, m)?)?;

    Ok(())
}
