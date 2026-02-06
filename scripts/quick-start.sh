#!/bin/bash
# Docktail Quick Start Script
# Automated setup for development and testing

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_step() {
    echo -e "${BLUE}==>${NC} $1"
}

print_success() {
    echo -e "${GREEN}✓${NC} $1"
}

print_error() {
    echo -e "${RED}✗${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}!${NC} $1"
}

# Check prerequisites
check_prerequisites() {
    print_step "Checking prerequisites..."
    
    if ! command -v docker &> /dev/null; then
        print_error "Docker is not installed. Please install Docker first."
        exit 1
    fi
    print_success "Docker found"
    
    if ! command -v docker compose &> /dev/null; then
        print_error "Docker Compose is not installed. Please install Docker Compose v2."
        exit 1
    fi
    print_success "Docker Compose found"
    
    # Check if ports are available
    if lsof -Pi :8080 -sTCP:LISTEN -t >/dev/null 2>&1; then
        print_warning "Port 8080 is already in use. Stop the process or change the port."
        read -p "Continue anyway? (y/n) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
}

# Generate certificates
generate_certs() {
    if [ -d "certs" ] && [ -f "certs/ca.crt" ]; then
        print_success "Certificates already exist"
        return 0
    fi
    
    print_step "Generating mTLS certificates..."
    
    # Try using Docker Compose first
    if docker compose --profile setup run --rm cert-generator 2>/dev/null; then
        print_success "Certificates generated"
        return 0
    fi
    
    # Fallback: Generate certs directly using the script
    print_warning "Docker Compose cert generation failed, using direct script..."
    
    # Create certs directory
    mkdir -p certs
    
    # Run the generation script in a temporary Alpine container
    docker run --rm -v "$SCRIPT_DIR/certs:/certs" -v "$SCRIPT_DIR/crates/agent/scripts:/scripts:ro" alpine:latest sh /scripts/generate-certs.sh
    
    if [ -f "certs/ca.crt" ]; then
        print_success "Certificates generated successfully"
    else
        print_error "Failed to generate certificates"
        exit 1
    fi
}

# Build images
build_images() {
    print_step "Building Docker images (this may take a few minutes)..."
    docker compose build --parallel
    print_success "Images built successfully"
}

# Start services
start_services() {
    print_step "Starting Docktail services..."
    docker compose up -d
    
    # Wait for cluster to be ready
    print_step "Waiting for cluster API to be ready..."
    for i in {1..30}; do
        if curl -s http://localhost:8080/graphiql > /dev/null 2>&1; then
            print_success "Cluster API is ready!"
            break
        fi
        if [ $i -eq 30 ]; then
            print_error "Cluster API failed to start. Check logs with: docker compose logs cluster"
            exit 1
        fi
        sleep 1
    done
}

# Show status
show_status() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo -e "${GREEN}🎉 Docktail is running!${NC}"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo -e "${BLUE}📊 Services:${NC}"
    docker compose ps --format "table {{.Name}}\t{{.Status}}\t{{.Ports}}"
    echo ""
    echo -e "${BLUE}🌐 Endpoints:${NC}"
    echo "  • GraphiQL:     http://localhost:8080/graphiql"
    echo "  • GraphQL API:  http://localhost:8080/graphql"
    echo "  • Metrics:      http://localhost:8080/metrics"
    echo "  • Agent 1 gRPC: localhost:50051"
    echo "  • Agent 2 gRPC: localhost:50052"
    echo ""
    echo -e "${BLUE}📦 Test Containers:${NC}"
    echo "  • test-app-1:   Logs every 2s"
    echo "  • test-app-2:   Logs every 3s"
    echo "  • test-app-3:   Logs every 5s"
    echo ""
    echo -e "${BLUE}🔧 Quick Commands:${NC}"
    echo "  • View logs:    docker compose logs -f cluster"
    echo "  • Stop all:     docker compose down"
    echo "  • Restart:      docker compose restart"
    echo "  • View status:  docker compose ps"
    echo ""
    echo -e "${BLUE}📚 Documentation:${NC}"
    echo "  • GraphQL API:  GRAPHQL_API.md"
    echo "  • Deployment:   DEPLOYMENT.md"
    echo "  • Operations:   crates/cluster/RUNBOOK.md"
    echo ""
    echo -e "${YELLOW}💡 Try this GraphQL query:${NC}"
    echo ""
    echo "Open http://localhost:8080/graphiql and run:"
    echo ""
    echo "  query {"
    echo "    agents {"
    echo "      id"
    echo "      name"
    echo "      status"
    echo "      containerCount"
    echo "    }"
    echo "    containers {"
    echo "      id"
    echo "      name"
    echo "      state"
    echo "    }"
    echo "  }"
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
}

# Main execution
main() {
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  🐳 Docktail Quick Start"
    echo "  Distributed Docker Log Streaming System"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    
    check_prerequisites
    generate_certs
    build_images
    start_services
    show_status
}

# Handle script arguments
case "${1:-}" in
    stop)
        print_step "Stopping all services..."
        docker compose down
        print_success "All services stopped"
        ;;
    restart)
        print_step "Restarting services..."
        docker compose restart
        print_success "Services restarted"
        show_status
        ;;
    rebuild)
        print_step "Rebuilding and restarting..."
        docker compose down
        docker compose build --no-cache
        docker compose up -d
        print_success "Rebuild complete"
        show_status
        ;;
    logs)
        docker compose logs -f "${2:-cluster}"
        ;;
    clean)
        print_warning "This will remove all containers, volumes, and certificates!"
        read -p "Are you sure? (y/n) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            docker compose down -v
            rm -rf certs/
            print_success "Cleanup complete"
        fi
        ;;
    status)
        docker compose ps
        ;;
    help|--help|-h)
        echo "Usage: ./quick-start.sh [command]"
        echo ""
        echo "Commands:"
        echo "  (none)    Start the full stack (default)"
        echo "  stop      Stop all services"
        echo "  restart   Restart all services"
        echo "  rebuild   Rebuild images and restart"
        echo "  logs      View logs (default: cluster)"
        echo "  status    Show service status"
        echo "  clean     Remove everything (containers, volumes, certs)"
        echo "  help      Show this help message"
        echo ""
        echo "Examples:"
        echo "  ./quick-start.sh              # Start everything"
        echo "  ./quick-start.sh logs agent1  # View agent1 logs"
        echo "  ./quick-start.sh stop         # Stop all services"
        ;;
    *)
        main
        ;;
esac
