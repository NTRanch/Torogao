"""Deterministic float64 rotation helpers for the mocap pipeline.

Quaternions are stored as (x, y, z, w) — the glTF order, matching
torogao-motion-clip/2. All functions are pure and allocation-simple;
no randomness, no platform-dependent branches beyond IEEE float64.
"""
import numpy as np

EPS = 1e-12


def normalize(v):
    n = float(np.linalg.norm(v))
    if n < EPS:
        return np.zeros_like(v)
    return v / n


def quat_from_matrix(R):
    """Rotation matrix (3x3) -> quaternion (x, y, z, w), canonical sign (w >= 0)."""
    t = float(np.trace(R))
    if t > 0.0:
        s = np.sqrt(t + 1.0) * 2.0
        w = 0.25 * s
        x = (R[2, 1] - R[1, 2]) / s
        y = (R[0, 2] - R[2, 0]) / s
        z = (R[1, 0] - R[0, 1]) / s
    elif R[0, 0] > R[1, 1] and R[0, 0] > R[2, 2]:
        s = np.sqrt(1.0 + R[0, 0] - R[1, 1] - R[2, 2]) * 2.0
        w = (R[2, 1] - R[1, 2]) / s
        x = 0.25 * s
        y = (R[0, 1] + R[1, 0]) / s
        z = (R[0, 2] + R[2, 0]) / s
    elif R[1, 1] > R[2, 2]:
        s = np.sqrt(1.0 + R[1, 1] - R[0, 0] - R[2, 2]) * 2.0
        w = (R[0, 2] - R[2, 0]) / s
        x = (R[0, 1] + R[1, 0]) / s
        y = 0.25 * s
        z = (R[1, 2] + R[2, 1]) / s
    else:
        s = np.sqrt(1.0 + R[2, 2] - R[0, 0] - R[1, 1]) * 2.0
        w = (R[1, 0] - R[0, 1]) / s
        x = (R[0, 2] + R[2, 0]) / s
        y = (R[1, 2] + R[2, 1]) / s
        z = 0.25 * s
    q = np.array([x, y, z, w], dtype=np.float64)
    q = q / np.linalg.norm(q)
    if q[3] < 0.0:
        q = -q
    return q


def matrix_from_quat(q):
    x, y, z, w = q
    return np.array([
        [1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
        [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
        [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)],
    ], dtype=np.float64)


def quat_mul(a, b):
    """Hamilton product a*b for (x, y, z, w) quaternions."""
    ax, ay, az, aw = a
    bx, by, bz, bw = b
    return np.array([
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ], dtype=np.float64)


def quat_slerp(a, b, t):
    """Shortest-arc slerp; deterministic lerp fallback near parallel."""
    d = float(np.dot(a, b))
    if d < 0.0:
        b = -b
        d = -d
    if d > 0.9995:
        q = a + t * (b - a)
        return q / np.linalg.norm(q)
    theta = np.arccos(min(1.0, d))
    s = np.sin(theta)
    wa = np.sin((1.0 - t) * theta) / s
    wb = np.sin(t * theta) / s
    return wa * a + wb * b


def frame_y_along(direction, ref):
    """Orthonormal basis (columns X, Y, Z) with Y along `direction` and Z
    as close to `ref` as orthogonality allows. Falls back to a fixed
    axis when direction and ref are near-parallel. Deterministic."""
    y = normalize(direction)
    z = ref - y * float(np.dot(ref, y))
    if np.linalg.norm(z) < 1e-6:
        # pick the world axis least aligned with y
        axes = [np.array([1.0, 0, 0]), np.array([0.0, 1, 0]), np.array([0.0, 0, 1.0])]
        ref2 = min(axes, key=lambda a: abs(float(np.dot(a, y))))
        z = ref2 - y * float(np.dot(ref2, y))
    z = normalize(z)
    x = np.cross(y, z)
    return np.stack([x, y, z], axis=1)


def sign_continuity(quats):
    """Flip quaternion signs along time so consecutive dots are >= 0."""
    out = quats.copy()
    for i in range(1, len(out)):
        if float(np.dot(out[i], out[i - 1])) < 0.0:
            out[i] = -out[i]
    return out
