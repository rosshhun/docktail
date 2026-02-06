#!/bin/sh
set -e

CERTS_DIR="/certs"

# Check if certificates already exist
if [ -f "$CERTS_DIR/agent.crt" ] && [ -f "$CERTS_DIR/agent.key" ] && [ -f "$CERTS_DIR/ca.crt" ]; then
    echo "✓ Certificates already exist, skipping generation"
    exit 0
fi

echo "🔐 Generating TLS certificates for Docktail Agent..."

# Install OpenSSL
apk add --no-cache openssl

# Create certs directory
mkdir -p "$CERTS_DIR"
cd "$CERTS_DIR"

# Generate CA private key and certificate
echo "  📜 Generating CA certificate..."
openssl req -x509 -newkey rsa:4096 -days 365 -nodes \
  -keyout ca.key -out ca.crt \
  -subj "/CN=Docktail CA/O=Docktail/C=US" \
  2>/dev/null

# Generate Agent server private key
echo "  🔑 Generating Agent server key..."
openssl genrsa -out agent.key 4096 2>/dev/null

# Generate Agent certificate signing request
echo "  📝 Creating Agent CSR..."
openssl req -new -key agent.key -out agent.csr \
  -subj "/CN=docktail-agent/O=Docktail/C=US" \
  2>/dev/null

# Sign Agent certificate with CA
echo "  ✍️  Signing Agent certificate..."
openssl x509 -req -in agent.csr -CA ca.crt -CAkey ca.key \
  -CAcreateserial -out agent.crt -days 365 \
  -extfile <(printf "subjectAltName=DNS:docktail-agent,DNS:localhost,IP:127.0.0.1") \
  2>/dev/null

# Generate client certificate for testing
echo "  🔑 Generating client certificate..."
openssl genrsa -out client.key 4096 2>/dev/null
openssl req -new -key client.key -out client.csr \
  -subj "/CN=docktail-client/O=Docktail/C=US" \
  2>/dev/null
openssl x509 -req -in client.csr -CA ca.crt -CAkey ca.key \
  -CAcreateserial -out client.crt -days 365 \
  2>/dev/null

# Generate Yaak-friendly client certificate (PKCS#12 format)
echo "  📦 Generating PKCS#12 certificate for Yaak..."
openssl pkcs12 -export -out client.p12 \
  -inkey client.key -in client.crt -certfile ca.crt \
  -passout pass:docktail \
  2>/dev/null

# Clean up temporary files
rm -f agent.csr client.csr ca.srl

# Set appropriate permissions
chmod 644 *.crt *.p12
chmod 600 *.key

echo ""
echo "✅ Certificate generation complete!"
echo ""
echo "📁 Generated files:"
echo "  - ca.crt         (CA certificate - use in Yaak)"
echo "  - agent.crt      (Agent server certificate)"
echo "  - agent.key      (Agent server private key)"
echo "  - client.crt     (Client certificate)"
echo "  - client.key     (Client private key)"
echo "  - client.p12     (PKCS#12 bundle for Yaak, password: docktail)"
echo ""
echo "🔧 For Yaak configuration:"
echo "  1. Use client.p12 with password 'docktail'"
echo "  2. Or use client.crt + client.key + ca.crt"
echo ""
