# Use Alpine Linux as the base image
FROM alpine:3.21

# Install required dependencies
RUN apk add --no-cache \
    git=2.47.1-r0 \
    nodejs=22.11.0-r1 \
    yarn=1.22.22-r1 
    rust=1.83.0-r0 \
    cargo=1.83.0-r0 \

# # Install Yarn globally
# RUN npm install -g yarn

# Set the working directory
WORKDIR /usr/src/app

# Copy the current directory contents into the container at /usr/src/app
COPY . .

# Expose the port the app runs on (optional)
# EXPOSE 8080
