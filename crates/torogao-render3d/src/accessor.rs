//! glTF accessor decoding (dense + sparse) over the `gltf`
//! crate's parsed document. The document structure — nodes,
//! skins, materials, meshes — comes from the `gltf` crate;
//! this module owns the byte-level decode because the sparse
//! morph accessors are rung 1's load-bearing data path and
//! their packing into the renderer's CSR layout must be
//! auditable line by line (gap analysis R2).
//!
//! Rules implemented, per the glTF 2.0 specification:
//! interleaved buffer views (byteStride), accessor
//! byteOffset, sparse substitution over a zero (or dense)
//! base, and normalized-integer conversion for colour, joint
//! and weight data.

use gltf::accessor::{DataType, Dimensions};
use gltf::buffer::Data as BufferData;
use gltf::{Accessor, Document};

use crate::error::RenderError;

fn err(index: usize, detail: impl Into<String>) -> RenderError {
    RenderError::Accessor {
        index,
        detail: detail.into(),
    }
}

/// Bytes per component for a data type.
fn component_size(data_type: DataType) -> usize {
    match data_type {
        DataType::I8 | DataType::U8 => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::U32 | DataType::F32 => 4,
    }
}

/// Read one component as f64 from little-endian bytes,
/// applying glTF normalized conversion when requested.
fn read_component(bytes: &[u8], data_type: DataType, normalized: bool) -> Result<f64, RenderError> {
    let raw: f64 = match data_type {
        DataType::I8 => f64::from(bytes[0] as i8),
        DataType::U8 => f64::from(bytes[0]),
        DataType::I16 => f64::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        DataType::U16 => f64::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        DataType::U32 => f64::from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        DataType::F32 => f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
    };
    if !normalized || data_type == DataType::F32 || data_type == DataType::U32 {
        return Ok(raw);
    }
    let converted = match data_type {
        DataType::I8 => (raw / 127.0).max(-1.0),
        DataType::U8 => raw / 255.0,
        DataType::I16 => (raw / 32767.0).max(-1.0),
        DataType::U16 => raw / 65535.0,
        _ => raw,
    };
    Ok(converted)
}

/// Slice the bytes an accessor (or a sparse indices/values
/// block) draws from: buffer view + offset, bounds-checked.
fn accessor_bytes<'a>(
    document: &Document,
    buffers: &'a [BufferData],
    view_index: usize,
    offset: usize,
    accessor_index: usize,
) -> Result<&'a [u8], RenderError> {
    let view = document
        .views()
        .nth(view_index)
        .ok_or_else(|| err(accessor_index, format!("buffer view {view_index} missing")))?;
    let buffer = buffers
        .get(view.buffer().index())
        .ok_or_else(|| err(accessor_index, "buffer index out of range"))?;
    let start = view.offset() + offset;
    let end = start + view.length();
    buffer
        .get(start..end)
        .ok_or_else(|| err(accessor_index, "buffer view range out of bounds"))
}

/// Decode an accessor to `count * components` f64 values,
/// expanding sparse storage over its (possibly absent) base.
pub fn read_f64s<'a>(
    document: &'a Document,
    buffers: &[BufferData],
    accessor: &Accessor<'a>,
) -> Result<Vec<f64>, RenderError> {
    let index = accessor.index();
    let components = accessor.dimensions().multiplicity();
    let expected_dims = match accessor.dimensions() {
        Dimensions::Scalar => 1,
        Dimensions::Vec2 => 2,
        Dimensions::Vec3 => 3,
        Dimensions::Vec4 | Dimensions::Mat2 => 4,
        Dimensions::Mat3 => 9,
        Dimensions::Mat4 => 16,
    };
    assert_eq!(components, expected_dims);
    let count = accessor.count();
    let data_type = accessor.data_type();
    let comp_bytes = component_size(data_type);
    let elem_bytes = comp_bytes * components;
    let mut out = vec![0.0_f64; count * components];

    // Dense base (absent for purely sparse accessors).
    if let Some(view) = accessor.view() {
        let stride = view.stride().unwrap_or(elem_bytes);
        if stride < elem_bytes {
            return Err(err(index, "buffer view stride smaller than element"));
        }
        let bytes = accessor_bytes(document, buffers, view.index(), accessor.offset(), index)?;
        for i in 0..count {
            let at = i * stride;
            let slice = bytes
                .get(at..at + elem_bytes)
                .ok_or_else(|| err(index, "element out of bounds"))?;
            for c in 0..components {
                out[i * components + c] =
                    read_component(&slice[c * comp_bytes..], data_type, accessor.normalized())?;
            }
        }
    }

    // Sparse substitution.
    if let Some(sparse) = accessor.sparse() {
        let indices = sparse.indices();
        let values = sparse.values();
        let sparse_count = sparse.count();
        let idx_type = match indices.index_type() {
            gltf::accessor::sparse::IndexType::U8 => DataType::U8,
            gltf::accessor::sparse::IndexType::U16 => DataType::U16,
            gltf::accessor::sparse::IndexType::U32 => DataType::U32,
        };
        let idx_bytes = component_size(idx_type);
        let idx_data = accessor_bytes(
            document,
            buffers,
            indices.view().index(),
            indices.offset(),
            index,
        )?;
        let val_data = accessor_bytes(
            document,
            buffers,
            values.view().index(),
            values.offset(),
            index,
        )?;
        for s in 0..sparse_count {
            let idx_slice = idx_data
                .get(s * idx_bytes..s * idx_bytes + idx_bytes)
                .ok_or_else(|| err(index, "sparse index out of bounds"))?;
            let target = read_component(idx_slice, idx_type, false)? as usize;
            if target >= count {
                return Err(err(
                    index,
                    format!("sparse index {target} exceeds count {count}"),
                ));
            }
            let val_slice = val_data
                .get(s * elem_bytes..s * elem_bytes + elem_bytes)
                .ok_or_else(|| err(index, "sparse value out of bounds"))?;
            for c in 0..components {
                out[target * components + c] = read_component(
                    &val_slice[c * comp_bytes..],
                    data_type,
                    accessor.normalized(),
                )?;
            }
        }
    }
    Ok(out)
}

/// Decode an accessor of f32 data (positions, normals,
/// colours, weights) to `count` rows of `N` components.
pub fn read_rows<'a, const N: usize>(
    document: &'a Document,
    buffers: &[BufferData],
    accessor: &Accessor<'a>,
) -> Result<Vec<[f32; N]>, RenderError> {
    let flat = read_f64s(document, buffers, accessor)?;
    if flat.len() % N != 0 {
        return Err(err(
            accessor.index(),
            format!("component count {} not divisible by {N}", flat.len()),
        ));
    }
    let mut out = Vec::with_capacity(flat.len() / N);
    for chunk in flat.chunks_exact(N) {
        let mut row = [0.0_f32; N];
        for (dst, src) in row.iter_mut().zip(chunk.iter()) {
            *dst = *src as f32;
        }
        out.push(row);
    }
    Ok(out)
}

/// Decode a joint-index accessor to u16 rows of 4. Joint
/// indices arrive as U8 or U16 per the specification.
pub fn read_joints<'a>(
    document: &'a Document,
    buffers: &[BufferData],
    accessor: &Accessor<'a>,
) -> Result<Vec<[u16; 4]>, RenderError> {
    let flat = read_f64s(document, buffers, accessor)?;
    if flat.len() % 4 != 0 {
        return Err(err(accessor.index(), "joint data not a multiple of 4"));
    }
    let mut out = Vec::with_capacity(flat.len() / 4);
    for chunk in flat.chunks_exact(4) {
        let mut row = [0_u16; 4];
        for (dst, src) in row.iter_mut().zip(chunk.iter()) {
            if *src < 0.0 || *src > f64::from(u16::MAX) {
                return Err(err(accessor.index(), "joint index out of u16 range"));
            }
            *dst = *src as u16;
        }
        out.push(row);
    }
    Ok(out)
}

/// Decode an index accessor to u32 (U16/U32/U8 sources).
pub fn read_indices<'a>(
    document: &'a Document,
    buffers: &[BufferData],
    accessor: &Accessor<'a>,
) -> Result<Vec<u32>, RenderError> {
    let flat = read_f64s(document, buffers, accessor)?;
    let mut out = Vec::with_capacity(flat.len());
    for value in flat {
        if value < 0.0 || value > f64::from(u32::MAX) {
            return Err(err(accessor.index(), "index out of u32 range"));
        }
        out.push(value as u32);
    }
    Ok(out)
}
