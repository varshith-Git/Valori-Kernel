# Publishing Valori to PyPI

Complete guide to publish the `valori` Python package to PyPI.

---

## 📋 Prerequisites

1. **PyPI Account**
   - Create account at https://pypi.org/account/register/
   - Create account at https://test.pypi.org/account/register/ (for testing)

2. **API Tokens**
   ```bash
   # Go to https://pypi.org/manage/account/token/
   # Create token with scope: "Entire account"
   # Save it securely!
   ```

3. **Install Publishing Tools**
   ```bash
   pip install build twine
   ```

---

## 🚀 Publishing Steps

### Step 1: Update Version

Edit `python/pyproject.toml`:
```toml
[project]
version = "0.1.0"  # Increment for each release
```

### Step 2: Build Package

```bash
cd python

# Clean previous builds
rm -rf dist/ build/ *.egg-info

# Build wheel and source distribution
python -m build
```

This creates:
- `dist/valori-0.1.0-py3-none-any.whl` (wheel)
- `dist/valori-0.1.0.tar.gz` (source)

### Step 3: Test on TestPyPI (recommended)

```bash
# Upload to test repository
python -m twine upload --repository testpypi dist/*

# When prompted:
# Username: __token__
# Password: <your-test-pypi-token>
```

**Test installation**:
```bash
pip install --index-url https://test.pypi.org/simple/ valori
```

### Step 4: Upload to PyPI

```bash
# Upload to production PyPI
python -m twine upload dist/*

# When prompted:
# Username: __token__
# Password: <your-pypi-token>
```

### Step 5: Verify

```bash
# Install from PyPI
pip install valori

# Test imports
python -c "from valori.adapters import ValoriAdapter; print('✅ Success!')"
```

---

## 🔄 Using GitHub Actions (Automated)

### Option A: Manual Trigger

Create `.github/workflows/publish-pypi.yml`:

```yaml
name: Publish to PyPI

on:
  workflow_dispatch:
    inputs:
      version:
        description: 'Version to publish (e.g., 0.1.0)'
        required: true

jobs:
  publish:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      
      - name: Set up Python
        uses: actions/setup-python@v4
        with:
          python-version: '3.11'
      
      - name: Install dependencies
        run: |
          python -m pip install --upgrade pip
          pip install build twine
      
      - name: Build package
        run: |
          cd python
          python -m build
      
      - name: Publish to PyPI
        env:
          TWINE_USERNAME: __token__
          TWINE_PASSWORD: ${{ secrets.PYPI_API_TOKEN }}
        run: |
          cd python
          python -m twine upload dist/*
```

**Setup**:
1. Go to GitHub repo → Settings → Secrets → Actions
2. Add secret: `PYPI_API_TOKEN` = your PyPI token
3. Go to Actions → Publish to PyPI → Run workflow

### Option B: Automated on Tag

```yaml
on:
  push:
    tags:
      - 'v*'  # Triggers on v0.1.0, v0.2.0, etc.
```

**Usage**:
```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## 📦 Package Structure

```
python/
├── pyproject.toml          # Package metadata
├── README.md               # PyPI description
├── MANIFEST.in            # Include/exclude files
├── LICENSE                # License file
└── valori/
    ├── __init__.py
    ├── protocol.py
    ├── memory.py
    ├── local.py
    ├── remote.py
    └── adapters/
        ├── __init__.py
        ├── base.py
        ├── langchain.py
        ├── langchain_vectorstore.py
        ├── llamaindex.py
        └── utils.py
```

---

## ✅ Pre-Publication Checklist

- [ ] Version number updated in `pyproject.toml`
- [ ] README.md is complete and accurate
- [ ] LICENSE file included
- [ ] All code tested locally
- [ ] Optional dependencies work (langchain, llamaindex)
- [ ] Examples run successfully
- [ ] No sensitive data in code
- [ ] GitHub repository URL correct

---

## 🔖 Version Guidelines

Follow [Semantic Versioning](https://semver.org/):
- `0.1.0` - Initial beta release
- `0.1.1` - Bug fixes
- `0.2.0` - New features (backward compatible)
- `1.0.0` - Stable release

---

## 📝 Release Notes Template

When publishing, create GitHub release:

```markdown
## valori 0.1.0

### 🎉 Initial Release!

Valori Python client with LangChain and LlamaIndex support.

**Features:**
- ✅ LangChain VectorStore integration
- ✅ LlamaIndex VectorStore integration
- ✅ Deterministic search with cross-platform guarantees
- ✅ Crash recovery support
- ✅ Cryptographic state proofs

**Installation:**
```bash
pip install valori[all]
```

**Quick Start:**
See [Python Usage Guide](docs/python-usage-guide.md)

**Examples:**
- [LangChain RAG](examples/langchain_example.py)
- [LlamaIndex Chat](examples/llamaindex_example.py)
```

---

## 🐛 Troubleshooting

### Error: "File already exists"
- You can't re-upload same version
- Increment version number in `pyproject.toml`

### Error: "Invalid credentials"
- Check PyPI token is correct
- Use `__token__` as username (yes, literally that)

### Error: "Package name already taken"
- Valori should be available
- If not, use `valori-db` or similar

---

## 🎯 Post-Publication

1. **Test Installation**
   ```bash
   pip install valori
   python -c "from valori.adapters import ValoriAdapter"
   ```

2. **Update Documentation**
   - Add PyPI badge to main README
   - Update installation instructions

3. **Announce**
   - Tweet/LinkedIn about the release
   - Post on relevant communities (r/MachineLearning, etc.)

---

## 📊 Monitoring

Check PyPI stats:
- https://pypi.org/project/valori/
- Download statistics
- User feedback in issues

---

**Ready to publish?** Follow the steps above! 🚀
