FROM golang:1.26-alpine AS build
ENV GOMAXPROCS=2 GOFLAGS=-p=1 GOWORK=off
WORKDIR /src
COPY go.mod go.sum ./
COPY cmd ./cmd
COPY internal ./internal
RUN CGO_ENABLED=0 go build -trimpath -ldflags='-s -w' -o /spectator ./cmd/spectator

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /spectator /spectator
ENV SPECTATOR_LISTEN_ADDRESS=127.0.0.1:3002
USER nonroot:nonroot
ENTRYPOINT ["/spectator"]
