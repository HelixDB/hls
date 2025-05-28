# Publishing the HelixQL Extension to VS Code Marketplace

## Prerequisites

1. **Azure DevOps Account**: Sign up at https://dev.azure.com
2. **Personal Access Token**: Create one with Marketplace > Manage permissions
3. **Publisher Account**: Create at https://marketplace.visualstudio.com/manage

## Step-by-Step Publishing Process

### 1. Create Azure DevOps Personal Access Token

1. Go to https://dev.azure.com
2. Click your profile → Personal Access Tokens
3. Click "New Token"
4. Configure:
   - **Name**: "VS Code Extension Publishing"
   - **Organization**: All accessible organizations
   - **Expiration**: 1 year
   - **Scopes**: Custom defined → **Marketplace** → **Manage**
5. Click "Create" and **COPY THE TOKEN** (you won't see it again!)

### 2. Create Publisher Account

1. Go to https://marketplace.visualstudio.com/manage
2. Sign in with your Microsoft account
3. Click "Create publisher"
4. Fill in:
   - **Publisher ID**: e.g., `your-username-helix` (this goes in URLs)
   - **Display Name**: e.g., "Your Name"
   - **Description**: Brief description

### 3. Update package.json

Replace these placeholders in `package.json`:

```json
{
  "publisher": "your-actual-publisher-id",
  "repository": {
    "url": "https://github.com/your-username/your-repo"
  },
  "bugs": {
    "url": "https://github.com/your-username/your-repo/issues"
  },
  "homepage": "https://github.com/your-username/your-repo#readme"
}
```

### 4. Login to vsce

```bash
npx vsce login your-actual-publisher-id
```

When prompted, paste your Personal Access Token.

### 5. Package and Publish

```bash
# First, compile and package
npm run compile
npm run package

# Then publish
npx vsce publish
```

### 6. Verify Publication

1. Go to https://marketplace.visualstudio.com/
2. Search for "helix query language"
3. Your extension should appear!

## Alternative: Manual Upload

If automatic publishing doesn't work:

1. Package the extension: `npm run package`
2. Go to https://marketplace.visualstudio.com/manage
3. Click your publisher name
4. Click "New extension"
5. Upload the `.vsix` file

## Version Updates

To publish updates:

1. Update version in `package.json`
2. Run: `npx vsce publish`

Or use automatic version bumping:
- `npx vsce publish patch` (0.1.9 → 0.1.10)
- `npx vsce publish minor` (0.1.9 → 0.2.0)
- `npx vsce publish major` (0.1.9 → 1.0.0)

## Troubleshooting

### Common Issues

1. **"Publisher not found"**: Make sure you've created the publisher at marketplace.visualstudio.com
2. **"Invalid token"**: Generate a new Personal Access Token with Marketplace > Manage permissions
3. **"Repository field missing"**: Update package.json with your GitHub repo URL

### Getting Help

- VS Code Extension Publishing Guide: https://code.visualstudio.com/api/working-with-extensions/publishing-extension
- Azure DevOps Personal Access Tokens: https://docs.microsoft.com/en-us/azure/devops/organizations/accounts/use-personal-access-tokens-to-authenticate 