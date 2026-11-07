# Provenance of the reference data

Every file here holds numbers derived from a source. No source file, figure or page is kept in the repository.

| File | Origin |
|---|---|
| `ds_reference.toml` | The reattachment length 6.26 with uncertainty 0.10 step heights, from the NASA Turbulence Modeling Resource page for the two-dimensional backward-facing step and the Driver and Seegmiller paper it cites. The geometry and the Reynolds number are the page's reference conditions. The tolerances are initial settings with their justification in the file. |
| `ds_profiles.csv` | Derived from a download of https://tmbwg.github.io/turbmodels/Backstep_validation/profiles.exp.dat (10341 bytes), source sha256 `23e8b44db52bfc36c66af6b743a4080c34f0165eb213a3db29c0914e12cf598d`. Columns taken: the height `y` (in step heights) and the mean streamwise velocity `u` (in reference velocities) of the blocks for x/H = 1, 4, 6 and 10, written with six decimals. The block for x/H = -4 and the other columns were not kept. No file is copied. |
| `armaly.toml` | The geometry and the Reynolds-number definition are from section 2 of Armaly, Durst, Pereira and Schonung (1983), J. Fluid Mech. 127, 473-496, DOI 10.1017/S0022112083002839. The tolerances are initial settings with their justification in the file. The method of the readings and their uncertainty (0.5 step heights) are stated in the file's `method`. |
| `armaly_reattachment.csv` | Read by eye from Fig. 4 (page 479) of a supplied copy of the paper (sha256 of the copy `67deca1c593fee1a5eb2c76bbb215f52f61c2fe3a005488a7c9397b156adafef`), at Re = 100 to 400 in steps of 50, against the frame and the axis ticks of the page rendered at 500 to 1000 dpi, and cross-checked against the measured curve of Fig. 14 (page 490). The two figures agree within 0.15 step heights. No page image, figure or file is kept. |
